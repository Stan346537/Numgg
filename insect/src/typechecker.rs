use std::collections::{HashMap, HashSet};

use crate::arithmetic::{Exponent, Power, Rational};
use crate::dimension::DimensionRegistry;
use crate::ffi::ArityRange;
use crate::registry::{BaseRepresentation, BaseRepresentationFactor, RegistryError};
use crate::typed_ast::{self, Type};
use crate::{ast, ffi, prefixes};

use num_traits::{FromPrimitive, Zero};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TypeCheckError {
    #[error("Unknown identifier '{0}'.")]
    UnknownIdentifier(String),

    #[error("Unknown function '{0}'.")]
    UnknownFunction(String),

    #[error("Incompatible dimensions in {0}:\n    {1}: {2}\n    {3}: {4}")]
    IncompatibleDimensions(
        String,
        &'static str,
        BaseRepresentation,
        &'static str,
        BaseRepresentation,
    ),

    #[error("Got dimension {0}, but exponent must be dimensionless.")]
    NonScalarExponent(BaseRepresentation),

    #[error("Unsupported expression in const-evaluation of exponent: {0}.")]
    UnsupportedConstEvalExpression(&'static str),

    #[error("Division by zero in dimension exponent")]
    DivisionByZeroInConstEvalExpression,

    #[error("{0}")]
    RegistryError(RegistryError),

    #[error("Incompatible alternative expressions have been provided for dimension '{0}'")]
    IncompatibleAlternativeDimensionExpression(String),

    #[error("Function or procedure '{callable_name}' called with {num_args} arguments(s), but needs {}..{}", arity.start(), arity.end())]
    // TODO: better formatting of the arity range (e.g. in case it includes just one number)
    WrongArity {
        callable_name: String,
        arity: ArityRange,
        num_args: usize,
    },

    #[error("'{0}' can not be used as a type parameter because it is also an existing dimension identifier.")]
    TypeParameterNameClash(String),

    #[error("Could not infer the type parameters {0} in the function call '{1}'.")]
    CanNotInferTypeParameters(String, String),

    #[error("Multiple unresolved generic parameters in a single function parameter type are not (yet) supported. Consider reordering the function parameters")]
    MultipleUnresolvedTypeParameters,

    #[error("Foreign function definition '{0}' needs a return type annotation.")]
    ForeignFunctionNeedsReturnTypeAnnotation(String),
}

type Result<T> = std::result::Result<T, TypeCheckError>;

fn to_rational_exponent(exponent_f64: f64) -> Exponent {
    Rational::from_f64(exponent_f64).unwrap() // TODO
}

/// Evaluates a limited set of expressions *at compile time*. This is needed to
/// support type checking of expressions like `(2 * meter)^(2*3 - 4)` where we
/// need to know not just the *type* but also the *value* of the exponent.
fn evaluate_const_expr(expr: &typed_ast::Expression) -> Result<Exponent> {
    match expr {
        typed_ast::Expression::Scalar(n) => Ok(to_rational_exponent(n.to_f64())),
        typed_ast::Expression::Negate(ref expr, _) => Ok(-evaluate_const_expr(expr)?),
        typed_ast::Expression::BinaryOperator(op, lhs_expr, rhs_expr, _) => {
            let lhs = evaluate_const_expr(lhs_expr)?;
            let rhs = evaluate_const_expr(rhs_expr)?;
            match op {
                typed_ast::BinaryOperator::Add => Ok(lhs + rhs),
                typed_ast::BinaryOperator::Sub => Ok(lhs - rhs),
                typed_ast::BinaryOperator::Mul => Ok(lhs * rhs),
                typed_ast::BinaryOperator::Div => {
                    if rhs == Rational::zero() {
                        Err(TypeCheckError::DivisionByZeroInConstEvalExpression)
                    } else {
                        Ok(lhs / rhs)
                    }
                }
                typed_ast::BinaryOperator::Power => {
                    if rhs.is_integer() {
                        Ok(lhs.pow(rhs.to_integer() as i32)) // TODO: dangerous cast
                    } else {
                        Err(TypeCheckError::UnsupportedConstEvalExpression(
                            "exponentiation with non-integer exponent",
                        ))
                    }
                }
                typed_ast::BinaryOperator::ConvertTo => {
                    Err(TypeCheckError::UnsupportedConstEvalExpression("conversion"))
                }
            }
        }
        typed_ast::Expression::Identifier(_, _) => {
            Err(TypeCheckError::UnsupportedConstEvalExpression("identifier"))
        }
        typed_ast::Expression::FunctionCall(_, _, _) => Err(
            TypeCheckError::UnsupportedConstEvalExpression("function call"),
        ),
    }
}

#[derive(Clone, Default)]
pub struct TypeChecker {
    identifiers: HashMap<String, Type>,
    function_signatures: HashMap<String, (Vec<String>, Vec<Type>, Type)>,
    registry: DimensionRegistry,
}

impl TypeChecker {
    fn type_for_identifier(&self, name: &str) -> Result<&Type> {
        self.identifiers
            .get(name)
            .ok_or_else(|| TypeCheckError::UnknownIdentifier(name.into()))
    }

    fn add_identifiers_for_unit(&mut self, name: &str, type_: &Type) -> Result<()> {
        self.identifiers.insert(name.into(), type_.clone());
        for (prefix_long, _, _) in prefixes::prefixes() {
            self.identifiers.insert(
                format!(
                    "{prefix_long}{name}",
                    prefix_long = prefix_long,
                    name = name
                ),
                type_.clone(),
            );
        }

        Ok(())
    }

    pub(crate) fn check_expression(&self, ast: ast::Expression) -> Result<typed_ast::Expression> {
        Ok(match ast {
            ast::Expression::Scalar(n) => typed_ast::Expression::Scalar(n),
            ast::Expression::Identifier(name) => {
                let type_ = self.type_for_identifier(&name)?.clone();

                typed_ast::Expression::Identifier(name, type_)
            }
            ast::Expression::Negate(expr) => {
                let checked_expr = self.check_expression(*expr)?;
                let type_ = checked_expr.get_type();
                typed_ast::Expression::Negate(Box::new(checked_expr), type_)
            }
            ast::Expression::BinaryOperator(op, lhs, rhs) => {
                let lhs = self.check_expression(*lhs)?;
                let rhs = self.check_expression(*rhs)?;

                let get_type_and_assert_equality = || {
                    let lhs_type = lhs.get_type();
                    let rhs_type = rhs.get_type();
                    if lhs_type != rhs_type {
                        Err(TypeCheckError::IncompatibleDimensions(
                            "binary operator".into(),
                            " left hand side",
                            lhs_type,
                            "right hand side",
                            rhs_type,
                        ))
                    } else {
                        Ok(lhs_type)
                    }
                };

                let type_ = match op {
                    typed_ast::BinaryOperator::Add => get_type_and_assert_equality()?,
                    typed_ast::BinaryOperator::Sub => get_type_and_assert_equality()?,
                    typed_ast::BinaryOperator::Mul => lhs.get_type().multiply(rhs.get_type()),
                    typed_ast::BinaryOperator::Div => lhs.get_type().divide(rhs.get_type()),
                    typed_ast::BinaryOperator::Power => {
                        let exponent_type = rhs.get_type();
                        if exponent_type != Type::unity() {
                            return Err(TypeCheckError::NonScalarExponent(exponent_type));
                        }

                        let base_type = lhs.get_type();
                        if base_type == Type::unity() {
                            // Skip evaluating the exponent if the lhs is a scalar. This allows
                            // for arbitrary (decimal) exponents, if the base is a scalar.

                            base_type
                        } else {
                            let exponent = evaluate_const_expr(&rhs)?;
                            base_type.power(exponent)
                        }
                    }
                    typed_ast::BinaryOperator::ConvertTo => get_type_and_assert_equality()?,
                };

                typed_ast::Expression::BinaryOperator(op, Box::new(lhs), Box::new(rhs), type_)
            }
            ast::Expression::FunctionCall(function_name, args) => {
                let (type_parameters, parameter_types, return_type) = self
                    .function_signatures
                    .get(&function_name)
                    .ok_or_else(|| TypeCheckError::UnknownFunction(function_name.clone()))?;

                if parameter_types.len() != args.len() {
                    return Err(TypeCheckError::WrongArity {
                        callable_name: function_name.clone(),
                        arity: parameter_types.len()..=parameter_types.len(),
                        num_args: args.len(),
                    });
                }

                let arguments_checked = args
                    .into_iter()
                    .map(|a| self.check_expression(a))
                    .collect::<Result<Vec<_>>>()?;
                let argument_types = arguments_checked.iter().map(|e| e.get_type());

                let mut substitutions: Vec<(String, Type)> = vec![];

                let substitute = |substitutions: &[(String, Type)], type_: &Type| -> Type {
                    let mut result_type = type_.clone();
                    for (name, substituted_type) in substitutions {
                        if let Some(factor @ BaseRepresentationFactor(_, exp)) = type_
                            .clone() // TODO: remove this .clone() somehow?
                            .iter()
                            .find(|BaseRepresentationFactor(n, _)| n == name)
                        {
                            result_type = result_type
                                .divide(Type::from_factor((*factor).clone()))
                                .multiply(substituted_type.clone().power(*exp));
                        }
                    }
                    result_type
                };

                for (idx, (parameter_type, argument_type)) in
                    parameter_types.iter().zip(argument_types).enumerate()
                {
                    let mut parameter_type = substitute(&substitutions, parameter_type);

                    let remaining_generic_subtypes: Vec<_> = parameter_type
                        .iter()
                        .filter(|BaseRepresentationFactor(name, _)| type_parameters.contains(name))
                        .collect();

                    if remaining_generic_subtypes.len() > 1 {
                        return Err(TypeCheckError::MultipleUnresolvedTypeParameters);
                    }

                    if let Some(&generic_subtype_factor) = remaining_generic_subtypes.first() {
                        let generic_subtype = Type::from_factor(generic_subtype_factor.clone());

                        // The type of the idx-th parameter of the called function has a generic type
                        // parameter inside. We can now instantiate that generic parameter by solving
                        // the equation "parameter_type == argument_type" for the generic parameter.
                        // In order to do this, let's assume `generic_subtype = D^alpha`, then we have
                        //
                        //                                parameter_type == argument_type
                        //    parameter_type / generic_subtype * D^alpha == argument_type
                        //                                       D^alpha == argument_type / (parameter_type / generic_subtype)
                        //                                             D == [argument_type / (parameter_type / generic_subtype)]^(1/alpha)
                        //

                        let alpha = Rational::from_integer(1) / generic_subtype_factor.1;
                        let d = argument_type
                            .clone()
                            .divide(parameter_type.clone().divide(generic_subtype))
                            .power(alpha);

                        // We can now substitute that generic parameter in all subsequent expressions
                        substitutions.push((generic_subtype_factor.0.clone(), d));

                        parameter_type = substitute(&substitutions, &parameter_type);
                    }

                    if parameter_type != argument_type {
                        return Err(TypeCheckError::IncompatibleDimensions(
                            format!(
                                "argument {num} of function call to '{name}'",
                                num = idx + 1,
                                name = function_name
                            ),
                            "parameter type",
                            parameter_type.clone(),
                            " argument type",
                            argument_type,
                        ));
                    }
                }

                if substitutions.len() != type_parameters.len() {
                    let parameters: HashSet<String> = type_parameters.iter().cloned().collect();
                    let inferred_parameters: HashSet<String> =
                        substitutions.iter().map(|t| t.0.clone()).collect();

                    let remaining: Vec<_> = (&parameters - &inferred_parameters)
                        .iter()
                        .cloned()
                        .collect();

                    return Err(TypeCheckError::CanNotInferTypeParameters(
                        function_name.clone(),
                        remaining.join(", "),
                    ));
                }

                let return_type = substitute(&substitutions, return_type);

                typed_ast::Expression::FunctionCall(function_name, arguments_checked, return_type)
            }
        })
    }

    pub fn check_statement(&mut self, ast: ast::Statement) -> Result<typed_ast::Statement> {
        Ok(match ast {
            ast::Statement::Expression(expr) => {
                typed_ast::Statement::Expression(self.check_expression(expr)?)
            }
            ast::Statement::DeclareVariable(name, expr, optional_dexpr) => {
                let expr = self.check_expression(expr)?;
                let type_deduced = expr.get_type();

                if let Some(ref dexpr) = optional_dexpr {
                    let type_specified = self
                        .registry
                        .get_base_representation(dexpr)
                        .map_err(TypeCheckError::RegistryError)?;
                    if type_deduced != type_specified {
                        return Err(TypeCheckError::IncompatibleDimensions(
                            "variable declaration".into(),
                            "specified dimension",
                            type_specified,
                            "   actual dimension",
                            type_deduced,
                        ));
                    }
                }
                self.identifiers.insert(name.clone(), type_deduced.clone());
                typed_ast::Statement::DeclareVariable(name, expr, type_deduced)
            }
            ast::Statement::DeclareBaseUnit(name, dexpr) => {
                let type_specified = self
                    .registry
                    .get_base_representation(&dexpr)
                    .map_err(TypeCheckError::RegistryError)?;
                self.add_identifiers_for_unit(&name, &type_specified)?;
                typed_ast::Statement::DeclareBaseUnit(name, type_specified)
            }
            ast::Statement::DeclareDerivedUnit(name, expr, optional_dexpr) => {
                // TODO: this is the *exact same code* that we have above for
                // variable declarations => deduplicate this somehow
                let expr = self.check_expression(expr)?;
                let type_deduced = expr.get_type();

                if let Some(ref dexpr) = optional_dexpr {
                    let type_specified = self
                        .registry
                        .get_base_representation(dexpr)
                        .map_err(TypeCheckError::RegistryError)?;
                    if type_deduced != type_specified {
                        return Err(TypeCheckError::IncompatibleDimensions(
                            "derived unit declaration".into(),
                            "specified dimension",
                            type_specified,
                            "   actual dimension",
                            type_deduced,
                        ));
                    }
                }
                self.add_identifiers_for_unit(&name, &type_deduced)?;
                typed_ast::Statement::DeclareDerivedUnit(name, expr)
            }
            ast::Statement::DeclareFunction(
                function_name,
                type_parameters,
                parameters,
                body,
                optional_return_type_dexpr,
            ) => {
                let mut typechecker_fn = self.clone();

                for type_parameter in &type_parameters {
                    match typechecker_fn.registry.add_base_dimension(type_parameter) {
                        Err(RegistryError::EntryExists(name)) => {
                            return Err(TypeCheckError::TypeParameterNameClash(name))
                        }
                        Err(err) => return Err(TypeCheckError::RegistryError(err)),
                        _ => {}
                    }
                }

                let mut typed_parameters = vec![];
                for (parameter, optional_dexpr) in parameters {
                    let parameter_type = typechecker_fn
                        .registry
                        .get_base_representation(
                            &optional_dexpr.expect("Parameter types can not be deduced."),
                        )
                        // TODO: add type inference, see https://github.com/sharkdp/insect-rs/issues/29
                        // TODO: once we add type inference, make sure that annotations are required for foreign functions
                        .map_err(TypeCheckError::RegistryError)?;
                    typechecker_fn
                        .identifiers
                        .insert(parameter.clone(), parameter_type.clone());
                    typed_parameters.push((parameter.clone(), parameter_type));
                }

                let return_type_specified = optional_return_type_dexpr
                    .map(|ref return_type_dexpr| {
                        typechecker_fn
                            .registry
                            .get_base_representation(return_type_dexpr)
                            .map_err(TypeCheckError::RegistryError)
                    })
                    .transpose()?;

                let body = body
                    .map(|expr| typechecker_fn.check_expression(expr))
                    .transpose()?;

                let return_type = if let Some(ref expr) = body {
                    let return_type_deduced = expr.get_type();
                    if let Some(return_type_specified) = return_type_specified {
                        if return_type_deduced != return_type_specified {
                            return Err(TypeCheckError::IncompatibleDimensions(
                                "function return type".into(),
                                "specified return type",
                                return_type_specified,
                                "   actual return type",
                                return_type_deduced,
                            ));
                        }
                    }
                    return_type_deduced
                } else {
                    return_type_specified.ok_or_else(|| {
                        TypeCheckError::ForeignFunctionNeedsReturnTypeAnnotation(
                            function_name.clone(),
                        )
                    })?
                };

                let parameter_types = typed_parameters.iter().map(|(_, t)| t.clone()).collect();
                self.function_signatures.insert(
                    function_name.clone(),
                    (type_parameters, parameter_types, return_type.clone()),
                );

                typed_ast::Statement::DeclareFunction(
                    function_name,
                    typed_parameters,
                    body,
                    return_type,
                )
            }
            ast::Statement::DeclareDimension(name, dexprs) => {
                if let Some(dexpr) = dexprs.first() {
                    self.registry
                        .add_derived_dimension(&name, dexpr)
                        .map_err(TypeCheckError::RegistryError)?;

                    let base_representation = self
                        .registry
                        .get_base_representation_for_name(&name)
                        .expect("we just inserted it");

                    for alternative_expr in &dexprs[1..] {
                        let alternative_base_representation = self
                            .registry
                            .get_base_representation(alternative_expr)
                            .map_err(TypeCheckError::RegistryError)?;
                        if alternative_base_representation != base_representation {
                            return Err(
                                TypeCheckError::IncompatibleAlternativeDimensionExpression(
                                    name.clone(),
                                ),
                            );
                        }
                    }
                } else {
                    self.registry
                        .add_base_dimension(&name)
                        .map_err(TypeCheckError::RegistryError)?;
                }
                typed_ast::Statement::DeclareDimension(name)
            }
            ast::Statement::ProcedureCall(kind, args) => {
                let procedure = ffi::procedures().get(&kind).unwrap();
                if !procedure.arity.contains(&args.len()) {
                    return Err(TypeCheckError::WrongArity {
                        callable_name: procedure.name.clone(),
                        arity: procedure.arity.clone(),
                        num_args: args.len(),
                    });
                }

                let checked_args = args
                    .into_iter()
                    .map(|e| self.check_expression(e))
                    .collect::<Result<Vec<typed_ast::Expression>>>()?;

                typed_ast::Statement::ProcedureCall(kind, checked_args)
            }
        })
    }

    pub fn check_statements(
        &mut self,
        statements: impl IntoIterator<Item = ast::Statement>,
    ) -> Result<Vec<typed_ast::Statement>> {
        let mut statements_checked = vec![];

        for statement in statements.into_iter() {
            statements_checked.push(self.check_statement(statement)?);
        }
        Ok(statements_checked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PRELUDE: &str = "
    dimension Scalar = 1
    dimension A
    dimension B
    dimension C = A * B
    unit a: A
    unit b: B
    unit c: C = a * b";

    fn base_type(name: &str) -> BaseRepresentation {
        BaseRepresentation::from_factor(BaseRepresentationFactor(
            name.into(),
            Rational::from_integer(1),
        ))
    }

    fn type_a() -> BaseRepresentation {
        base_type("A")
    }

    fn type_b() -> BaseRepresentation {
        base_type("B")
    }

    fn type_c() -> BaseRepresentation {
        type_a().multiply(type_b())
    }

    fn run_typecheck(input: &str) -> Result<typed_ast::Statement> {
        let code = &format!("{prelude}\n{input}", prelude = TEST_PRELUDE, input = input);
        let statements =
            crate::parser::parse(code).expect("No parse errors for inputs in this test suite");

        TypeChecker::default()
            .check_statements(statements)
            .map(|mut statements_checked| statements_checked.pop().unwrap())
    }

    fn assert_successful_typecheck(input: &str) {
        assert!(run_typecheck(input).is_ok());
    }

    fn get_typecheck_error(input: &str) -> TypeCheckError {
        if let Err(err) = dbg!(run_typecheck(input)) {
            err
        } else {
            panic!("Input was expected to yield a type check error");
        }
    }

    #[test]
    fn basic_arithmetic() {
        assert_successful_typecheck("2 a + a");
        assert_successful_typecheck("2 a - a");

        assert_successful_typecheck("a * b");
        assert_successful_typecheck("a / b");

        assert_successful_typecheck("a * b + 2 c");
        assert_successful_typecheck("c / a + b");

        assert!(matches!(
            get_typecheck_error("a + b"),
            TypeCheckError::IncompatibleDimensions(_, _, t1, _, t2) if t1 == type_a() && t2 == type_b()
        ));
    }

    #[test]
    fn power_operator_with_scalar_base() {
        assert_successful_typecheck("2^2");
        assert_successful_typecheck("2^(2^2)");

        assert!(matches!(
            get_typecheck_error("2^a"),
            TypeCheckError::NonScalarExponent(t) if t == type_a()
        ));
        assert!(matches!(
            get_typecheck_error("2^(c/b)"),
            TypeCheckError::NonScalarExponent(t) if t == type_a()
        ));
    }

    #[test]
    fn power_operator_with_dimensionful_base() {
        assert_successful_typecheck("a^2");
        assert_successful_typecheck("a^(2+3)");
        assert_successful_typecheck("a^(2-3)");
        assert_successful_typecheck("a^(2*3)");
        assert_successful_typecheck("a^(2/3)");
        assert_successful_typecheck("a^(2^3)");

        assert!(matches!(
            get_typecheck_error("a^b"),
            TypeCheckError::NonScalarExponent(t) if t == type_b()
        ));

        // TODO: if we add ("constexpr") constants later, it would be great to support those in exponents.
        assert!(matches!(
            get_typecheck_error("let x=2
                                 a^x"),
            TypeCheckError::UnsupportedConstEvalExpression(desc) if desc == "identifier"
        ));

        assert!(matches!(
            get_typecheck_error("a^(3/(1-1))"),
            TypeCheckError::DivisionByZeroInConstEvalExpression
        ));
    }

    #[test]
    fn variable_declarations() {
        assert_successful_typecheck(
            "let x: A = a
             let y: B = b",
        );
        assert_successful_typecheck("let x: C = a * b");
        assert_successful_typecheck("let x: C = 2 * a * b^2 / b");
        assert_successful_typecheck("let x: A^3 = a^20 * a^(-17)");

        assert_successful_typecheck("let x: A = c / b");

        assert!(matches!(
            get_typecheck_error("let x: A = b"),
            TypeCheckError::IncompatibleDimensions(_, _, t1, _, t2) if t1 == type_a() && t2 == type_b()
        ));
    }

    #[test]
    fn unit_declarations() {
        assert_successful_typecheck("unit my_c: C = a * b");
        assert_successful_typecheck("unit foo: A*B^2 = a b^2");

        assert!(matches!(
            get_typecheck_error("unit my_c: C = a"),
            TypeCheckError::IncompatibleDimensions(_, _, t1, _, t2) if t1 == type_c() && t2 == type_a()
        ));
    }

    #[test]
    fn function_declarations() {
        assert_successful_typecheck("fn f(x: A) -> A = x");
        assert_successful_typecheck("fn f(x: A) -> A·B = 2 * x * b");
        assert_successful_typecheck("fn f(x: A, y: B) -> C = x * y");

        assert_successful_typecheck("fn f(x: A) = x");

        assert!(matches!(
            get_typecheck_error("fn f(x: A, y: B) -> C = x / y"),
            TypeCheckError::IncompatibleDimensions(_, _, t1, _, t2) if t1 == type_c() && t2 == type_a().divide(type_b())
        ));

        assert!(matches!(
            get_typecheck_error("fn f(x: A) -> A = a\n\
                                 f(b)"),
            TypeCheckError::IncompatibleDimensions(_, _, t1, _, t2) if t1 == type_a() && t2 == type_b()
        ));
    }

    #[test]
    fn generics_basic() {
        assert_successful_typecheck(
            "
            fn f<D>(x: D) -> D = x
            f(2)
            f(2 a)
            ",
        );
        assert_successful_typecheck(
            "
            fn f<D>(x: D) -> D^2 = x*x
            f(2)
            f(2 a)
            ",
        );
        assert_successful_typecheck(
            "
            fn f<D0, D1>(x: D0, y: D1) -> D0/D1^2 = x/y^2
            f(2, 3)
            f(2 a, 2 b)
            ",
        );

        assert!(matches!(
            get_typecheck_error("fn f<T1, T2>(x: T1, y: T2) -> T2/T1 = x/y"),
            TypeCheckError::IncompatibleDimensions(_, _, t1, _, t2)
                if t1 == base_type("T2").divide(base_type("T1")) &&
                   t2 == base_type("T1").divide(base_type("T2"))
        ));
    }

    #[test]
    fn generics_multiple_unresolved_type_parameters() {
        assert!(matches!(
            get_typecheck_error(
                "
                fn foo<D1, D2>(x: D1*D2) = 1
                foo(2)
            "
            ),
            TypeCheckError::MultipleUnresolvedTypeParameters
        ));
    }

    #[test]
    fn generics_unused_type_parameter() {
        assert!(matches!(
            get_typecheck_error("
                fn foo<D0>(x: Scalar) -> Scalar = 1
                foo(2)
            "),
            TypeCheckError::CanNotInferTypeParameters(function_name, parameters) if function_name == "foo" && parameters == "D0"
        ));

        assert!(matches!(
            get_typecheck_error("
                fn foo<D0, D1>(x: D0, y: D0) -> Scalar = 1
                foo(2, 3)
            "),
            TypeCheckError::CanNotInferTypeParameters(function_name, parameters) if function_name == "foo" && parameters == "D1"
        ));

        assert!(matches!(
            get_typecheck_error("
                fn foo<D0, D1>(x: Scalar, y: Scalar) -> Scalar = 1
                foo(2, 3)
            "),
            TypeCheckError::CanNotInferTypeParameters(function_name, parameters) if function_name == "foo" && (parameters == "D1, D0" || parameters == "D0, D1")
        ));
    }

    #[test]
    fn generics_type_parameter_name_clash() {
        assert!(matches!(
            get_typecheck_error("
                dimension Existing
                fn f<Existing>(x: Existing) = 1
            "),
            TypeCheckError::TypeParameterNameClash(name) if name == "Existing"
        ));
    }

    #[test]
    fn unknown_identifier() {
        assert!(matches!(
            get_typecheck_error("a + d"),
            TypeCheckError::UnknownIdentifier(ident) if ident == "d"
        ));
    }

    #[test]
    fn unknown_function() {
        assert!(matches!(
            get_typecheck_error("foo(2)"),
            TypeCheckError::UnknownFunction(name) if name == "foo"
        ));
    }

    #[test]
    fn incompatible_alternative_dimension_expression() {
        assert!(matches!(
            get_typecheck_error(
                "# wrong alternative expression: C / B^2
                 dimension D = A / B = C / B^3"
            ),
            TypeCheckError::IncompatibleAlternativeDimensionExpression(t) if t == "D",
        ));
    }

    #[test]
    fn wrong_arity() {
        assert!(matches!(
            get_typecheck_error("
                fn f() = 1
                f(1)
            "),
            TypeCheckError::WrongArity{callable_name, arity, num_args: 1} if arity == (0..=0) && callable_name == "f"
        ));

        assert!(matches!(
            get_typecheck_error("
                fn f(x: Scalar) = x
                f()
            "),
            TypeCheckError::WrongArity{callable_name, arity, num_args: 0} if arity == (1..=1) && callable_name == "f"
        ));

        assert!(matches!(
            get_typecheck_error("
                fn f(x: Scalar) = x
                f(2, 3)
            "),
            TypeCheckError::WrongArity{callable_name, arity, num_args: 2} if arity == (1..=1) && callable_name == "f"
        ));
    }

    #[test]
    fn foreign_function_with_missing_return_type() {
        assert!(matches!(
            get_typecheck_error("fn sin(x: Scalar)"),
            TypeCheckError::ForeignFunctionNeedsReturnTypeAnnotation(name) if name == "sin"
        ));
    }

    #[test]
    fn arity_checks_in_procedure_calls() {
        assert!(matches!(
            get_typecheck_error("assert_eq(1)"),
            TypeCheckError::WrongArity{callable_name, arity, num_args: 1} if arity == (2..=3) && callable_name == "assert_eq"
        ));
        assert_successful_typecheck("assert_eq(1,2)");
        assert_successful_typecheck("assert_eq(1,2,3)");
        assert!(matches!(
            get_typecheck_error("assert_eq(1,2,3,4)"),
            TypeCheckError::WrongArity{callable_name, arity, num_args: 4} if arity == (2..=3) && callable_name == "assert_eq"
        ));
    }
}
