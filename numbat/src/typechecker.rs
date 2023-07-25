use std::collections::{HashMap, HashSet};

use crate::arithmetic::{Exponent, Power, Rational};
use crate::dimension::DimensionRegistry;
use crate::ffi::ArityRange;
use crate::name_resolution::LAST_RESULT_IDENTIFIERS;
use crate::registry::{BaseRepresentation, BaseRepresentationFactor, RegistryError};
use crate::span::Span;
use crate::typed_ast::{self, Type};
use crate::{ast, decorator, ffi};

use ast::DimensionExpression;
use num_traits::{FromPrimitive, Zero};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TypeCheckError {
    #[error("Unknown identifier '{1}'.")]
    UnknownIdentifier(Span, String, Option<String>),

    #[error("Unknown callable '{1}'.")]
    UnknownCallable(Span, String),

    #[error("{expected_name}: {expected_type}\n{actual_name}: {actual_type}")]
    IncompatibleDimensions {
        span_operation: Span,
        operation: String,
        span_expected: Span,
        expected_name: &'static str,
        expected_type: BaseRepresentation,
        span_actual: Span,
        actual_name: &'static str,
        actual_type: BaseRepresentation,
    },

    #[error("Exponents need to be dimensionless (got {1}).")]
    NonScalarExponent(Span, BaseRepresentation),

    #[error("Argument of factorial needs to be dimensionless (got {1}).")]
    NonScalarFactorialArgument(Span, BaseRepresentation),

    #[error("Unsupported expression in const-evaluation of exponent: {1}.")]
    UnsupportedConstEvalExpression(Span, &'static str),

    #[error("Division by zero in const. eval. expression")]
    DivisionByZeroInConstEvalExpression(Span),

    #[error("{0}")]
    RegistryError(RegistryError),

    #[error("Incompatible alternative expressions have been provided for dimension '{0}'")]
    IncompatibleAlternativeDimensionExpression(String, Span, Type, Span, Type),

    #[error("Function or procedure '{callable_name}' called with {num_args} arguments(s), but needs {}..{}", arity.start(), arity.end())]
    WrongArity {
        callable_span: Span,
        callable_name: String,
        callable_definition_span: Option<Span>,
        arity: ArityRange,
        num_args: usize,
    },

    #[error("'{1}' can not be used as a type parameter because it is also an existing dimension identifier.")]
    TypeParameterNameClash(Span, String),

    #[error("Could not infer the type parameters {3} in the function call '{2}'.")]
    CanNotInferTypeParameters(Span, Span, String, String),

    #[error("Multiple unresolved generic parameters in a single function parameter type are not (yet) supported. Consider reordering the function parameters")]
    MultipleUnresolvedTypeParameters(Span, Span),

    #[error("Foreign function definition (without body) '{1}' needs parameter and return type annotations.")]
    ForeignFunctionNeedsTypeAnnotations(Span, String),

    #[error("Unknown foreign function (without body) '{1}'")]
    UnknownForeignFunction(Span, String),
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
        typed_ast::Expression::Scalar(_, n) => Ok(to_rational_exponent(n.to_f64())),
        typed_ast::Expression::UnaryOperator(_, ast::UnaryOperator::Negate, ref expr, _) => {
            Ok(-evaluate_const_expr(expr)?)
        }
        e @ typed_ast::Expression::UnaryOperator(_, ast::UnaryOperator::Factorial, _, _) => Err(
            TypeCheckError::UnsupportedConstEvalExpression(e.full_span(), "factorial"),
        ),
        e @ typed_ast::Expression::BinaryOperator(_span_op, op, lhs_expr, rhs_expr, _) => {
            let lhs = evaluate_const_expr(lhs_expr)?;
            let rhs = evaluate_const_expr(rhs_expr)?;
            match op {
                typed_ast::BinaryOperator::Add => Ok(lhs + rhs),
                typed_ast::BinaryOperator::Sub => Ok(lhs - rhs),
                typed_ast::BinaryOperator::Mul => Ok(lhs * rhs),
                typed_ast::BinaryOperator::Div => {
                    if rhs == Rational::zero() {
                        Err(TypeCheckError::DivisionByZeroInConstEvalExpression(
                            e.full_span(),
                        ))
                    } else {
                        Ok(lhs / rhs)
                    }
                }
                typed_ast::BinaryOperator::Power => {
                    if rhs.is_integer() {
                        Ok(lhs.pow(rhs.to_integer() as i32)) // TODO: dangerous cast
                    } else {
                        Err(TypeCheckError::UnsupportedConstEvalExpression(
                            e.full_span(),
                            "exponentiation with non-integer exponent",
                        ))
                    }
                }
                typed_ast::BinaryOperator::ConvertTo => Err(
                    TypeCheckError::UnsupportedConstEvalExpression(e.full_span(), "conversion"),
                ),
            }
        }
        e @ typed_ast::Expression::Identifier(..) => Err(
            TypeCheckError::UnsupportedConstEvalExpression(e.full_span(), "variable"),
        ),
        e @ typed_ast::Expression::UnitIdentifier(..) => Err(
            TypeCheckError::UnsupportedConstEvalExpression(e.full_span(), "unit identifier"),
        ),
        e @ typed_ast::Expression::FunctionCall(_, _, _, _, _) => Err(
            TypeCheckError::UnsupportedConstEvalExpression(e.full_span(), "function call"),
        ),
    }
}

#[derive(Clone, Default)]
pub struct TypeChecker {
    identifiers: HashMap<String, Type>,
    function_signatures:
        HashMap<String, (Span, Vec<(Span, String)>, Vec<(Span, Type)>, bool, Type)>,
    registry: DimensionRegistry,
}

impl TypeChecker {
    fn type_for_identifier(&self, span: Span, name: &str) -> Result<&Type> {
        self.identifiers.get(name).ok_or_else(|| {
            let suggestion = self
                .identifiers
                .iter()
                .map(|(id, _)| id)
                .min_by_key(|id| strsim::damerau_levenshtein(id, name))
                .filter(|id| {
                    name.len() >= 3 && id.len() >= 2 && strsim::damerau_levenshtein(id, name) <= 3
                });
            TypeCheckError::UnknownIdentifier(span, name.into(), suggestion.cloned())
        })
    }

    pub(crate) fn check_expression(&self, ast: &ast::Expression) -> Result<typed_ast::Expression> {
        Ok(match ast {
            ast::Expression::Scalar(span, n) => typed_ast::Expression::Scalar(*span, n.clone()),
            ast::Expression::Identifier(span, name) => {
                let type_ = self.type_for_identifier(*span, name)?.clone();

                typed_ast::Expression::Identifier(*span, name.clone(), type_)
            }
            ast::Expression::UnitIdentifier(span, prefix, name, full_name) => {
                let type_ = self.type_for_identifier(*span, &name)?.clone();

                typed_ast::Expression::UnitIdentifier(
                    *span,
                    prefix.clone(),
                    name.clone(),
                    full_name.clone(),
                    type_,
                )
            }
            ast::Expression::UnaryOperator { op, expr, span_op } => {
                let checked_expr = self.check_expression(expr)?;
                let type_ = checked_expr.get_type();

                match *op {
                    ast::UnaryOperator::Factorial => {
                        if type_ != Type::unity() {
                            return Err(TypeCheckError::NonScalarFactorialArgument(
                                expr.full_span(),
                                type_,
                            ));
                        }
                    }
                    ast::UnaryOperator::Negate => {}
                }

                typed_ast::Expression::UnaryOperator(*span_op, *op, Box::new(checked_expr), type_)
            }
            ast::Expression::BinaryOperator {
                op,
                lhs,
                rhs,
                span_op,
            } => {
                let lhs_checked = self.check_expression(&lhs)?;
                let rhs_checked = self.check_expression(&rhs)?;

                let get_type_and_assert_equality = || {
                    let lhs_type = lhs_checked.get_type();
                    let rhs_type = rhs_checked.get_type();
                    if lhs_type != rhs_type {
                        let full_span = ast::Expression::BinaryOperator {
                            op: *op,
                            lhs: lhs.clone(),
                            rhs: rhs.clone(),
                            span_op: *span_op,
                        }
                        .full_span();
                        Err(TypeCheckError::IncompatibleDimensions {
                            span_operation: span_op.unwrap_or(full_span),
                            operation: match op {
                                typed_ast::BinaryOperator::Add => "addition".into(),
                                typed_ast::BinaryOperator::Sub => "subtraction".into(),
                                typed_ast::BinaryOperator::Mul => "multiplication".into(),
                                typed_ast::BinaryOperator::Div => "division".into(),
                                typed_ast::BinaryOperator::Power => "exponentiation".into(),
                                typed_ast::BinaryOperator::ConvertTo => "unit conversion".into(),
                            },
                            span_expected: lhs.full_span(),
                            expected_name: " left hand side",
                            expected_type: lhs_type,
                            span_actual: rhs.full_span(),
                            actual_name: "right hand side",
                            actual_type: rhs_type,
                        })
                    } else {
                        Ok(lhs_type)
                    }
                };

                let type_ = match op {
                    typed_ast::BinaryOperator::Add => get_type_and_assert_equality()?,
                    typed_ast::BinaryOperator::Sub => get_type_and_assert_equality()?,
                    typed_ast::BinaryOperator::Mul => {
                        lhs_checked.get_type() * rhs_checked.get_type()
                    }
                    typed_ast::BinaryOperator::Div => {
                        lhs_checked.get_type() / rhs_checked.get_type()
                    }
                    typed_ast::BinaryOperator::Power => {
                        let exponent_type = rhs_checked.get_type();
                        if exponent_type != Type::unity() {
                            return Err(TypeCheckError::NonScalarExponent(
                                rhs.full_span(),
                                exponent_type,
                            ));
                        }

                        let base_type = lhs_checked.get_type();
                        if base_type == Type::unity() {
                            // Skip evaluating the exponent if the lhs is a scalar. This allows
                            // for arbitrary (decimal) exponents, if the base is a scalar.

                            base_type
                        } else {
                            let exponent = evaluate_const_expr(&rhs_checked)?;
                            base_type.power(exponent)
                        }
                    }
                    typed_ast::BinaryOperator::ConvertTo => get_type_and_assert_equality()?,
                };

                typed_ast::Expression::BinaryOperator(
                    span_op.clone(),
                    op.clone(),
                    Box::new(lhs_checked),
                    Box::new(rhs_checked),
                    type_,
                )
            }
            ast::Expression::FunctionCall(span, full_span, function_name, args) => {
                let (
                    callable_definition_span,
                    type_parameters,
                    parameter_types,
                    is_variadic,
                    return_type,
                ) = self
                    .function_signatures
                    .get(function_name)
                    .ok_or_else(|| TypeCheckError::UnknownCallable(*span, function_name.clone()))?;

                let arity_range = if *is_variadic {
                    1..=usize::MAX
                } else {
                    parameter_types.len()..=parameter_types.len()
                };

                if !arity_range.contains(&args.len()) {
                    return Err(TypeCheckError::WrongArity {
                        callable_span: *span,
                        callable_name: function_name.clone(),
                        callable_definition_span: Some(*callable_definition_span),
                        arity: arity_range,
                        num_args: args.len(),
                    });
                }

                let arguments_checked = args
                    .iter()
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
                            result_type = result_type / Type::from_factor((*factor).clone())
                                * substituted_type.clone().power(*exp);
                        }
                    }
                    result_type
                };

                let mut parameter_types = parameter_types.clone();
                if *is_variadic {
                    // For a variadic function, we simply duplicate the parameter type
                    // N times, where N is the number of arguments given.
                    debug_assert!(parameter_types.len() == 1);

                    for _ in 1..argument_types.len() {
                        parameter_types.push(parameter_types[0].clone());
                    }
                }

                for (idx, ((parameter_span, parameter_type), argument_type)) in
                    parameter_types.iter().zip(argument_types).enumerate()
                {
                    let mut parameter_type = substitute(&substitutions, parameter_type);

                    let remaining_generic_subtypes: Vec<_> = parameter_type
                        .iter()
                        .filter(|BaseRepresentationFactor(name, _)| {
                            type_parameters.iter().any(|(_, n)| name == n)
                        })
                        .collect();

                    if remaining_generic_subtypes.len() > 1 {
                        return Err(TypeCheckError::MultipleUnresolvedTypeParameters(
                            *span,
                            *parameter_span,
                        ));
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
                        let d = (argument_type.clone()
                            / (parameter_type.clone() / generic_subtype))
                            .power(alpha);

                        // We can now substitute that generic parameter in all subsequent expressions
                        substitutions.push((generic_subtype_factor.0.clone(), d));

                        parameter_type = substitute(&substitutions, &parameter_type);
                    }

                    if parameter_type != argument_type {
                        return Err(TypeCheckError::IncompatibleDimensions {
                            span_operation: *span,
                            operation: format!(
                                "argument {num} of function call to '{name}'",
                                num = idx + 1,
                                name = function_name
                            ),
                            span_expected: parameter_types[idx].0,
                            expected_name: "parameter type",
                            expected_type: parameter_type.clone(),
                            span_actual: args[idx].full_span(),
                            actual_name: " argument type",
                            actual_type: argument_type,
                        });
                    }
                }

                if substitutions.len() != type_parameters.len() {
                    let parameters: HashSet<String> = type_parameters
                        .iter()
                        .map(|(_, name)| name)
                        .cloned()
                        .collect();
                    let inferred_parameters: HashSet<String> =
                        substitutions.iter().map(|t| t.0.clone()).collect();

                    let remaining: Vec<_> = (&parameters - &inferred_parameters)
                        .iter()
                        .cloned()
                        .collect();

                    return Err(TypeCheckError::CanNotInferTypeParameters(
                        *span,
                        *callable_definition_span,
                        function_name.clone(),
                        remaining.join(", "),
                    ));
                }

                let return_type = substitute(&substitutions, return_type);

                typed_ast::Expression::FunctionCall(
                    span.clone(),
                    full_span.clone(),
                    function_name.clone(),
                    arguments_checked,
                    return_type,
                )
            }
        })
    }

    pub fn check_statement(&mut self, ast: &ast::Statement) -> Result<typed_ast::Statement> {
        Ok(match ast {
            ast::Statement::Expression(expr) => {
                let checked_expr = self.check_expression(expr)?;
                for &identifier in LAST_RESULT_IDENTIFIERS {
                    self.identifiers
                        .insert(identifier.into(), checked_expr.get_type());
                }
                typed_ast::Statement::Expression(checked_expr)
            }
            ast::Statement::DefineVariable {
                identifier_span,
                identifier,
                expr,
                type_annotation,
            } => {
                let expr_checked = self.check_expression(expr)?;
                let type_deduced = expr_checked.get_type();

                if let Some(ref dexpr) = type_annotation {
                    let type_specified = self
                        .registry
                        .get_base_representation(dexpr)
                        .map_err(TypeCheckError::RegistryError)?;
                    if type_deduced != type_specified {
                        return Err(TypeCheckError::IncompatibleDimensions {
                            span_operation: *identifier_span,
                            operation: "variable definition".into(),
                            span_expected: dexpr.full_span(),
                            expected_name: "specified dimension",
                            expected_type: type_specified,
                            span_actual: expr.full_span(),
                            actual_name: "   actual dimension",
                            actual_type: type_deduced,
                        });
                    }
                }
                self.identifiers
                    .insert(identifier.clone(), type_deduced.clone());
                typed_ast::Statement::DefineVariable(identifier.clone(), expr_checked, type_deduced)
            }
            ast::Statement::DefineBaseUnit(_span, unit_name, dexpr, decorators) => {
                let type_specified = if let Some(dexpr) = dexpr {
                    self.registry
                        .get_base_representation(&dexpr)
                        .map_err(TypeCheckError::RegistryError)?
                } else {
                    use heck::ToUpperCamelCase;
                    // In a unit definition like 'unit pixel' without a specified type,
                    // we add a new type for the user
                    let type_name = unit_name.to_upper_camel_case();
                    self.registry
                        .add_base_dimension(&type_name)
                        .map_err(TypeCheckError::RegistryError)?
                };
                for (name, _) in decorator::name_and_aliases(&unit_name, &decorators) {
                    self.identifiers
                        .insert(name.clone(), type_specified.clone());
                }
                typed_ast::Statement::DefineBaseUnit(
                    unit_name.clone(),
                    decorators.clone(),
                    type_specified,
                )
            }
            ast::Statement::DefineDerivedUnit {
                identifier_span,
                identifier,
                expr,
                type_annotation_span,
                type_annotation,
                decorators,
            } => {
                // TODO: this is the *exact same code* that we have above for
                // variable definitions => deduplicate this somehow
                let expr_checked = self.check_expression(expr)?;
                let type_deduced = expr_checked.get_type();

                if let Some(ref dexpr) = type_annotation {
                    let type_specified = self
                        .registry
                        .get_base_representation(dexpr)
                        .map_err(TypeCheckError::RegistryError)?;
                    if type_deduced != type_specified {
                        return Err(TypeCheckError::IncompatibleDimensions {
                            span_operation: *identifier_span,
                            operation: "unit definition".into(),
                            span_expected: type_annotation_span.unwrap(),
                            expected_name: "specified dimension",
                            expected_type: type_specified,
                            span_actual: expr.full_span(),
                            actual_name: "   actual dimension",
                            actual_type: type_deduced,
                        });
                    }
                }
                for (name, _) in decorator::name_and_aliases(&identifier, &decorators) {
                    self.identifiers.insert(name.clone(), type_deduced.clone());
                }
                typed_ast::Statement::DefineDerivedUnit(
                    identifier.clone(),
                    expr_checked,
                    decorators.clone(),
                )
            }
            ast::Statement::DefineFunction {
                function_name_span,
                function_name,
                type_parameters,
                parameters,
                body,
                return_type_span,
                return_type_annotation,
            } => {
                let mut typechecker_fn = self.clone();
                let is_ffi_function = body.is_none();
                let mut type_parameters = type_parameters.clone();

                for (span, type_parameter) in &type_parameters {
                    match typechecker_fn.registry.add_base_dimension(&type_parameter) {
                        Err(RegistryError::EntryExists(name)) => {
                            return Err(TypeCheckError::TypeParameterNameClash(*span, name))
                        }
                        Err(err) => return Err(TypeCheckError::RegistryError(err)),
                        _ => {}
                    }
                }

                let mut typed_parameters = vec![];
                let mut is_variadic = false;
                let mut free_type_parameters = vec![];
                for (parameter_span, parameter, type_annotation, p_is_variadic) in parameters {
                    let parameter_type = if let Some(type_) = type_annotation {
                        typechecker_fn
                            .registry
                            .get_base_representation(&type_)
                            .map_err(TypeCheckError::RegistryError)?
                    } else if is_ffi_function {
                        return Err(TypeCheckError::ForeignFunctionNeedsTypeAnnotations(
                            *function_name_span,
                            function_name.clone(),
                        ));
                    } else {
                        let free_type_parameter =
                            format!("__T{num}", num = free_type_parameters.len());
                        free_type_parameters.push((parameter.clone(), free_type_parameter.clone()));

                        typechecker_fn
                            .registry
                            .add_base_dimension(&free_type_parameter)
                            .expect("double-underscore identifiers are only used internally");
                        type_parameters.push((parameter_span.clone(), free_type_parameter.clone()));
                        typechecker_fn
                            .registry
                            .get_base_representation(&DimensionExpression::Dimension(
                                parameter_span.clone(),
                                free_type_parameter,
                            ))
                            .map_err(TypeCheckError::RegistryError)?
                    };

                    typechecker_fn
                        .identifiers
                        .insert(parameter.clone(), parameter_type.clone());
                    typed_parameters.push((
                        *parameter_span,
                        parameter.clone(),
                        *p_is_variadic,
                        parameter_type,
                    ));

                    is_variadic |= p_is_variadic;
                }

                if free_type_parameters.len() > 0 {
                    // TODO: Perform type inference
                }

                let return_type_specified = return_type_annotation
                    .clone()
                    .map(|ref annotation| {
                        typechecker_fn
                            .registry
                            .get_base_representation(annotation)
                            .map_err(TypeCheckError::RegistryError)
                    })
                    .transpose()?;

                let body_checked = body
                    .clone()
                    .map(|expr| typechecker_fn.check_expression(&expr))
                    .transpose()?;

                let return_type = if let Some(ref expr) = body_checked {
                    let return_type_deduced = expr.get_type();
                    if let Some(return_type_specified) = return_type_specified {
                        if return_type_deduced != return_type_specified {
                            return Err(TypeCheckError::IncompatibleDimensions {
                                span_operation: *function_name_span,
                                operation: "function return type".into(),
                                span_expected: return_type_span.unwrap(),
                                expected_name: "specified return type",
                                expected_type: return_type_specified,
                                span_actual: body.as_ref().map(|b| b.full_span()).unwrap(),
                                actual_name: "   actual return type",
                                actual_type: return_type_deduced,
                            });
                        }
                    }
                    return_type_deduced
                } else {
                    if !ffi::functions().contains_key(function_name.as_str()) {
                        return Err(TypeCheckError::UnknownForeignFunction(
                            *function_name_span,
                            function_name.clone(),
                        ));
                    }

                    return_type_specified.ok_or_else(|| {
                        TypeCheckError::ForeignFunctionNeedsTypeAnnotations(
                            *function_name_span,
                            function_name.clone(),
                        )
                    })?
                };

                let parameter_types = typed_parameters
                    .iter()
                    .map(|(span, _, _, t)| (*span, t.clone()))
                    .collect();
                self.function_signatures.insert(
                    function_name.clone(),
                    (
                        *function_name_span,
                        type_parameters.clone(),
                        parameter_types,
                        is_variadic,
                        return_type.clone(),
                    ),
                );

                typed_ast::Statement::DefineFunction(
                    function_name.clone(),
                    typed_parameters,
                    body_checked,
                    return_type,
                )
            }
            ast::Statement::DefineDimension(name, dexprs) => {
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
                                    dexpr.full_span(),
                                    base_representation,
                                    alternative_expr.full_span(),
                                    alternative_base_representation,
                                ),
                            );
                        }
                    }
                } else {
                    self.registry
                        .add_base_dimension(&name)
                        .map_err(TypeCheckError::RegistryError)?;
                }
                typed_ast::Statement::DefineDimension(name.clone())
            }
            ast::Statement::ProcedureCall(span, kind, args) => {
                let procedure = ffi::procedures().get(&kind).unwrap();
                if !procedure.arity.contains(&args.len()) {
                    return Err(TypeCheckError::WrongArity {
                        callable_span: *span,
                        callable_name: procedure.name.clone(),
                        callable_definition_span: None,
                        arity: procedure.arity.clone(),
                        num_args: args.len(),
                    });
                }

                let checked_args = args
                    .into_iter()
                    .map(|e| self.check_expression(e))
                    .collect::<Result<Vec<typed_ast::Expression>>>()?;

                typed_ast::Statement::ProcedureCall(kind.clone(), checked_args)
            }
            ast::Statement::ModuleImport(_, _) => {
                unreachable!("Modules should have been inlined by now")
            }
        })
    }

    pub fn check_statements(
        &mut self,
        statements: impl IntoIterator<Item = ast::Statement>,
    ) -> Result<Vec<typed_ast::Statement>> {
        let mut statements_checked = vec![];

        for statement in statements.into_iter() {
            statements_checked.push(self.check_statement(&statement)?);
        }
        Ok(statements_checked)
    }
}

#[cfg(test)]
mod tests {
    use crate::parser::parse;
    use crate::prefix_transformer::Transformer;

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
        type_a() * type_b()
    }

    fn run_typecheck(input: &str) -> Result<typed_ast::Statement> {
        let code = &format!("{prelude}\n{input}", prelude = TEST_PRELUDE, input = input);
        let statements = parse(code, 0).expect("No parse errors for inputs in this test suite");
        let transformed_statements = Transformer::new()
            .transform(statements)
            .expect("No name resolution errors for inputs in this test suite");

        TypeChecker::default()
            .check_statements(transformed_statements)
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
            TypeCheckError::IncompatibleDimensions{expected_type, actual_type, ..} if expected_type == type_a() && actual_type == type_b()
        ));
    }

    #[test]
    fn power_operator_with_scalar_base() {
        assert_successful_typecheck("2^2");
        assert_successful_typecheck("2^(2^2)");

        assert!(matches!(
            get_typecheck_error("2^a"),
            TypeCheckError::NonScalarExponent(_, t) if t == type_a()
        ));
        assert!(matches!(
            get_typecheck_error("2^(c/b)"),
            TypeCheckError::NonScalarExponent(_, t) if t == type_a()
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
            TypeCheckError::NonScalarExponent(_, t) if t == type_b()
        ));

        // TODO: if we add ("constexpr") constants later, it would be great to support those in exponents.
        assert!(matches!(
            get_typecheck_error("let x=2
                                 a^x"),
            TypeCheckError::UnsupportedConstEvalExpression(_, desc) if desc == "variable"
        ));

        assert!(matches!(
            get_typecheck_error("a^(3/(1-1))"),
            TypeCheckError::DivisionByZeroInConstEvalExpression(_)
        ));
    }

    #[test]
    fn variable_definitions() {
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
            TypeCheckError::IncompatibleDimensions{expected_type, actual_type, ..} if expected_type == type_a() && actual_type == type_b()
        ));
    }

    #[test]
    fn unit_definitions() {
        assert_successful_typecheck("unit my_c: C = a * b");
        assert_successful_typecheck("unit foo: A*B^2 = a b^2");

        assert!(matches!(
            get_typecheck_error("unit my_c: C = a"),
            TypeCheckError::IncompatibleDimensions{expected_type, actual_type, ..} if expected_type == type_c() && actual_type == type_a()
        ));
    }

    #[test]
    fn function_definitions() {
        assert_successful_typecheck("fn f(x: A) -> A = x");
        assert_successful_typecheck("fn f(x: A) -> A·B = 2 * x * b");
        assert_successful_typecheck("fn f(x: A, y: B) -> C = x * y");

        assert_successful_typecheck("fn f(x: A) = x");

        assert!(matches!(
            get_typecheck_error("fn f(x: A, y: B) -> C = x / y"),
            TypeCheckError::IncompatibleDimensions{expected_type, actual_type, ..} if expected_type == type_c() && actual_type == type_a() / type_b()
        ));

        assert!(matches!(
            get_typecheck_error("fn f(x: A) -> A = a\n\
                                 f(b)"),
            TypeCheckError::IncompatibleDimensions{expected_type, actual_type, ..} if expected_type == type_a() && actual_type == type_b()
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
            TypeCheckError::IncompatibleDimensions{expected_type, actual_type, ..}
                if expected_type == base_type("T2") / base_type("T1") &&
                actual_type == base_type("T1") / base_type("T2")
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
            TypeCheckError::MultipleUnresolvedTypeParameters(..)
        ));
    }

    #[test]
    fn generics_unused_type_parameter() {
        assert!(matches!(
            get_typecheck_error("
                fn foo<D0>(x: Scalar) -> Scalar = 1
                foo(2)
            "),
            TypeCheckError::CanNotInferTypeParameters(_, _, function_name, parameters) if function_name == "foo" && parameters == "D0"
        ));

        assert!(matches!(
            get_typecheck_error("
                fn foo<D0, D1>(x: D0, y: D0) -> Scalar = 1
                foo(2, 3)
            "),
            TypeCheckError::CanNotInferTypeParameters(_, _, function_name, parameters) if function_name == "foo" && parameters == "D1"
        ));

        assert!(matches!(
            get_typecheck_error("
                fn foo<D0, D1>(x: Scalar, y: Scalar) -> Scalar = 1
                foo(2, 3)
            "),
            TypeCheckError::CanNotInferTypeParameters(_, _, function_name, parameters) if function_name == "foo" && (parameters == "D1, D0" || parameters == "D0, D1")
        ));
    }

    #[test]
    fn generics_type_parameter_name_clash() {
        assert!(matches!(
            get_typecheck_error("
                dimension Existing
                fn f<Existing>(x: Existing) = 1
            "),
            TypeCheckError::TypeParameterNameClash(_, name) if name == "Existing"
        ));
    }

    #[test]
    fn unknown_identifier() {
        assert!(matches!(
            get_typecheck_error("a + d"),
            TypeCheckError::UnknownIdentifier(_, ident, _) if ident == "d"
        ));
    }

    #[test]
    fn unknown_function() {
        assert!(matches!(
            get_typecheck_error("foo(2)"),
            TypeCheckError::UnknownCallable(_, name) if name == "foo"
        ));
    }

    #[test]
    fn incompatible_alternative_dimension_expression() {
        assert!(matches!(
            get_typecheck_error(
                "# wrong alternative expression: C / B^2
                 dimension D = A / B = C / B^3"
            ),
            TypeCheckError::IncompatibleAlternativeDimensionExpression(t, ..) if t == "D",
        ));
    }

    #[test]
    fn wrong_arity() {
        assert!(matches!(
            get_typecheck_error("
                fn f() = 1
                f(1)
            "),
            TypeCheckError::WrongArity{callable_span:_, callable_name, callable_definition_span: _, arity, num_args: 1} if arity == (0..=0) && callable_name == "f"
        ));

        assert!(matches!(
            get_typecheck_error("
                fn f(x: Scalar) = x
                f()
            "),
            TypeCheckError::WrongArity{callable_span:_, callable_name, callable_definition_span: _,  arity, num_args: 0} if arity == (1..=1) && callable_name == "f"
        ));

        assert!(matches!(
            get_typecheck_error("
                fn f(x: Scalar) = x
                f(2, 3)
            "),
            TypeCheckError::WrongArity{callable_span:_, callable_name, callable_definition_span: _,  arity, num_args: 2} if arity == (1..=1) && callable_name == "f"
        ));

        assert!(matches!(
            get_typecheck_error("
                fn mean<D>(xs: D…) -> D
                mean()
            "),
            TypeCheckError::WrongArity{callable_span:_, callable_name, callable_definition_span: _,  arity, num_args: 0} if arity == (1..=usize::MAX) && callable_name == "mean"
        ));
    }

    #[test]
    fn variadic_functions() {
        assert!(matches!(
            get_typecheck_error(
                "
                fn mean<D>(xs: D…) -> D
                mean(1 a, 1 b)
            "
            ),
            TypeCheckError::IncompatibleDimensions { .. }
        ));
    }

    #[test]
    fn foreign_function_with_missing_return_type() {
        assert!(matches!(
            get_typecheck_error("fn sin(x: Scalar)"),
            TypeCheckError::ForeignFunctionNeedsTypeAnnotations(_, name) if name == "sin"
        ));
    }

    #[test]
    fn unknown_foreign_function() {
        assert!(matches!(
            get_typecheck_error("fn foo(x: Scalar) -> Scalar"),
            TypeCheckError::UnknownForeignFunction(_, name) if name == "foo"
        ));
    }

    #[test]
    fn arity_checks_in_procedure_calls() {
        assert!(matches!(
            get_typecheck_error("assert_eq(1)"),
            TypeCheckError::WrongArity{callable_span:_, callable_name, callable_definition_span: _,  arity, num_args: 1} if arity == (2..=3) && callable_name == "assert_eq"
        ));
        assert_successful_typecheck("assert_eq(1,2)");
        assert_successful_typecheck("assert_eq(1,2,3)");
        assert!(matches!(
            get_typecheck_error("assert_eq(1,2,3,4)"),
            TypeCheckError::WrongArity{callable_span:_, callable_name, callable_definition_span: _,  arity, num_args: 4} if arity == (2..=3) && callable_name == "assert_eq"
        ));
    }
}
