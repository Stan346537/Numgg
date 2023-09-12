use itertools::Itertools;

use crate::ast::ProcedureKind;
pub use crate::ast::{BinaryOperator, DimensionExpression, UnaryOperator};
use crate::markup as m;
use crate::{
    decorator::Decorator, markup::Markup, number::Number, prefix::Prefix,
    prefix_parser::AcceptsPrefix, pretty_print::PrettyPrint, registry::BaseRepresentation,
    span::Span,
};

/// Dimension type
pub type DType = BaseRepresentation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Dimension(DType),
    Boolean,
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Dimension(d) => d.fmt(f),
            Type::Boolean => write!(f, "bool"),
        }
    }
}

impl PrettyPrint for Type {
    fn pretty_print(&self) -> Markup {
        match self {
            Type::Dimension(d) => m::type_identifier(d.to_string()), // TODO: properly pretty-print the type. ideally, look up the abbreviated name
            Type::Boolean => m::keyword("bool"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Scalar(Span, Number),
    Identifier(Span, String, Type),
    UnitIdentifier(Span, Prefix, String, String, Type),
    UnaryOperator(Span, UnaryOperator, Box<Expression>, Type),
    BinaryOperator(
        Option<Span>,
        BinaryOperator,
        Box<Expression>,
        Box<Expression>,
        Type,
    ),
    FunctionCall(Span, Span, String, Vec<Expression>, DType),
    Boolean(Span, bool),
    Condition(Span, Box<Expression>, Box<Expression>, Box<Expression>),
}

impl Expression {
    pub fn full_span(&self) -> Span {
        match self {
            Expression::Scalar(span, ..) => *span,
            Expression::Identifier(span, ..) => *span,
            Expression::UnitIdentifier(span, ..) => *span,
            Expression::UnaryOperator(span, _, expr, _) => span.extend(&expr.full_span()),
            Expression::BinaryOperator(span_op, _op, lhs, rhs, _) => {
                let mut span = lhs.full_span().extend(&rhs.full_span());
                if let Some(span_op) = span_op {
                    span = span.extend(span_op);
                }
                span
            }
            Expression::FunctionCall(_identifier_span, full_span, _, _, _) => *full_span,
            Expression::Boolean(span, _) => *span,
            Expression::Condition(span_if, _, _, then_expr) => {
                span_if.extend(&then_expr.full_span())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Expression(Expression),
    DefineVariable(String, Expression, Option<DimensionExpression>, Type),
    DefineFunction(
        String,
        Vec<String>, // type parameters
        Vec<(
            // parameter:
            Span,                        // span of the parameter
            String,                      // parameter name
            bool,                        // whether or not it is variadic
            Option<DimensionExpression>, // parameter type annotation
            Type,                        // parameter type
        )>,
        Option<Expression>,          // function body
        Option<DimensionExpression>, // return type annotation
        Type,                        // return type
    ),
    DefineDimension(String, Vec<DimensionExpression>),
    DefineBaseUnit(String, Vec<Decorator>, Option<DimensionExpression>, Type),
    DefineDerivedUnit(
        String,
        Expression,
        Vec<Decorator>,
        Option<DimensionExpression>,
    ),
    ProcedureCall(crate::ast::ProcedureKind, Vec<Expression>),
}

impl Expression {
    pub(crate) fn get_type(&self) -> Type {
        match self {
            Expression::Scalar(_, _) => Type::Dimension(DType::unity()),
            Expression::Identifier(_, _, type_) => type_.clone(),
            Expression::UnitIdentifier(_, _, _, _, _type) => _type.clone(),
            Expression::UnaryOperator(_, _, _, type_) => type_.clone(),
            Expression::BinaryOperator(_, _, _, _, type_) => type_.clone(),
            Expression::FunctionCall(_, _, _, _, type_) => Type::Dimension(type_.clone()),
            Expression::Boolean(_, _) => Type::Boolean,
            Expression::Condition(_, _, then, _) => then.get_type(),
        }
    }
}

fn accepts_prefix_markup(accepts_prefix: &Option<AcceptsPrefix>) -> Markup {
    if let Some(accepts_prefix) = accepts_prefix {
        m::operator(":")
            + m::space()
            + match accepts_prefix {
                AcceptsPrefix {
                    short: true,
                    long: true,
                } => m::keyword("both"),
                AcceptsPrefix {
                    short: true,
                    long: false,
                } => m::keyword("short"),
                AcceptsPrefix {
                    short: false,
                    long: true,
                } => m::keyword("long"),
                AcceptsPrefix {
                    short: false,
                    long: false,
                } => m::keyword("none"),
            }
    } else {
        Markup::default()
    }
}

fn decorator_markup(decorators: &Vec<Decorator>) -> Markup {
    let mut markup_decorators = Markup::default();
    for decorator in decorators {
        markup_decorators = markup_decorators
            + match decorator {
                Decorator::MetricPrefixes => m::decorator("@metric_prefixes"),
                Decorator::BinaryPrefixes => m::decorator("@binary_prefixes"),
                Decorator::Aliases(names) => {
                    m::decorator("@aliases")
                        + m::operator("(")
                        + Itertools::intersperse(
                            names.iter().map(|(name, accepts_prefix)| {
                                m::unit(name) + accepts_prefix_markup(accepts_prefix)
                            }),
                            m::operator(", "),
                        )
                        .sum()
                        + m::operator(")")
                }
            }
            + m::nl();
    }
    markup_decorators
}

fn annotation_or_actual_type(annotation: &Option<DimensionExpression>, type_: &Type) -> Markup {
    if let Some(annotation) = annotation {
        annotation.pretty_print()
    } else {
        type_.pretty_print()
    }
}

impl PrettyPrint for Statement {
    fn pretty_print(&self) -> Markup {
        match self {
            Statement::DefineVariable(identifier, expr, type_annotation, type_) => {
                m::keyword("let")
                    + m::space()
                    + m::identifier(identifier)
                    + m::operator(":")
                    + m::space()
                    + annotation_or_actual_type(type_annotation, type_)
                    + m::space()
                    + m::operator("=")
                    + m::space()
                    + expr.pretty_print()
            }
            Statement::DefineFunction(
                function_name,
                type_parameters,
                parameters,
                body,
                return_type_annotation,
                return_type,
            ) => {
                let markup_type_parameters = if type_parameters.is_empty() {
                    Markup::default()
                } else {
                    m::operator("<")
                        + Itertools::intersperse(
                            type_parameters.iter().map(m::type_identifier),
                            m::operator(", "),
                        )
                        .sum()
                        + m::operator(">")
                };

                let markup_parameters = Itertools::intersperse(
                    parameters
                        .iter()
                        .map(|(_span, name, is_variadic, type_annotation, type_)| {
                            m::identifier(name)
                                + m::operator(":")
                                + m::space()
                                + annotation_or_actual_type(type_annotation, type_)
                                + if *is_variadic {
                                    m::operator("…")
                                } else {
                                    Markup::default()
                                }
                        }),
                    m::operator(", "),
                )
                .sum();

                let markup_return_type = m::space()
                    + m::operator("->")
                    + m::space()
                    + annotation_or_actual_type(return_type_annotation, return_type);

                m::keyword("fn")
                    + m::space()
                    + m::identifier(function_name)
                    + markup_type_parameters
                    + m::operator("(")
                    + markup_parameters
                    + m::operator(")")
                    + markup_return_type
                    + body
                        .as_ref()
                        .map(|e| m::space() + m::operator("=") + m::space() + e.pretty_print())
                        .unwrap_or_default()
            }
            Statement::Expression(expr) => expr.pretty_print(),
            Statement::DefineDimension(identifier, dexprs) if dexprs.is_empty() => {
                m::keyword("dimension") + m::space() + m::type_identifier(identifier)
            }
            Statement::DefineDimension(identifier, dexprs) => {
                m::keyword("dimension")
                    + m::space()
                    + m::type_identifier(identifier)
                    + m::space()
                    + m::operator("=")
                    + m::space()
                    + Itertools::intersperse(
                        dexprs.iter().map(|d| d.pretty_print()),
                        m::space() + m::operator("=") + m::space(),
                    )
                    .sum()
            }
            Statement::DefineBaseUnit(identifier, decorators, type_annotation, type_) => {
                decorator_markup(decorators)
                    + m::keyword("unit")
                    + m::space()
                    + m::unit(identifier)
                    + m::operator(":")
                    + m::space()
                    + annotation_or_actual_type(type_annotation, type_)
            }
            Statement::DefineDerivedUnit(identifier, expr, decorators, type_annotation) => {
                decorator_markup(decorators)
                    + m::keyword("unit")
                    + m::space()
                    + m::unit(identifier)
                    + m::operator(":")
                    + m::space()
                    + annotation_or_actual_type(type_annotation, &expr.get_type())
                    + m::space()
                    + m::operator("=")
                    + m::space()
                    + expr.pretty_print()
            }
            Statement::ProcedureCall(kind, args) => {
                let identifier = match kind {
                    ProcedureKind::Print => "print",
                    ProcedureKind::AssertEq => "assert_eq",
                    ProcedureKind::Type => "type",
                };
                m::identifier(identifier)
                    + m::operator("(")
                    + Itertools::intersperse(
                        args.iter().map(|a| a.pretty_print()),
                        m::operator(",") + m::space(),
                    )
                    .sum()
                    + m::operator(")")
            }
        }
    }
}

fn pretty_scalar(Number(n): Number) -> Markup {
    m::value(format!("{n}"))
}

fn with_parens(expr: &Expression) -> Markup {
    match expr {
        Expression::Scalar(..)
        | Expression::Identifier(..)
        | Expression::UnitIdentifier(..)
        | Expression::FunctionCall(..)
        | Expression::Boolean(..) => expr.pretty_print(),
        Expression::UnaryOperator { .. }
        | Expression::BinaryOperator { .. }
        | Expression::Condition(..) => m::operator("(") + expr.pretty_print() + m::operator(")"),
    }
}

/// Add parens, if needed -- liberal version, can not be used for exponentiation.
fn with_parens_liberal(expr: &Expression) -> Markup {
    match expr {
        Expression::BinaryOperator(_, BinaryOperator::Mul, lhs, rhs, _type)
            if matches!(**lhs, Expression::Scalar(..))
                && matches!(**rhs, Expression::UnitIdentifier(..)) =>
        {
            expr.pretty_print()
        }
        _ => with_parens(expr),
    }
}

fn pretty_print_binop(op: &BinaryOperator, lhs: &Expression, rhs: &Expression) -> Markup {
    match op {
        BinaryOperator::ConvertTo => {
            // never needs parens, it has the lowest precedence:
            lhs.pretty_print() + op.pretty_print() + rhs.pretty_print()
        }
        BinaryOperator::Mul => match (lhs, rhs) {
            (
                Expression::Scalar(_, s),
                Expression::UnitIdentifier(_, prefix, _name, full_name, _type),
            ) => {
                // Fuse multiplication of a scalar and a unit to a quantity
                pretty_scalar(*s)
                    + m::space()
                    + m::unit(format!("{}{}", prefix.as_string_long(), full_name))
            }
            (Expression::Scalar(_, s), Expression::Identifier(_, name, _type)) => {
                // Fuse multiplication of a scalar and identifier
                pretty_scalar(*s) + m::space() + m::identifier(name)
            }
            _ => {
                let add_parens_if_needed = |expr: &Expression| {
                    if matches!(
                        expr,
                        Expression::BinaryOperator(_, BinaryOperator::Power, ..)
                            | Expression::BinaryOperator(_, BinaryOperator::Mul, ..)
                    ) {
                        expr.pretty_print()
                    } else {
                        with_parens_liberal(expr)
                    }
                };

                add_parens_if_needed(lhs) + op.pretty_print() + add_parens_if_needed(rhs)
            }
        },
        BinaryOperator::Div => {
            let lhs_add_parens_if_needed = |expr: &Expression| {
                if matches!(
                    expr,
                    Expression::BinaryOperator(_, BinaryOperator::Power, ..)
                        | Expression::BinaryOperator(_, BinaryOperator::Mul, ..)
                ) {
                    expr.pretty_print()
                } else {
                    with_parens_liberal(expr)
                }
            };
            let rhs_add_parens_if_needed = |expr: &Expression| {
                if matches!(
                    expr,
                    Expression::BinaryOperator(_, BinaryOperator::Power, ..)
                ) {
                    expr.pretty_print()
                } else {
                    with_parens_liberal(expr)
                }
            };

            lhs_add_parens_if_needed(lhs) + op.pretty_print() + rhs_add_parens_if_needed(rhs)
        }
        BinaryOperator::Add => {
            let add_parens_if_needed = |expr: &Expression| {
                if matches!(
                    expr,
                    Expression::BinaryOperator(_, BinaryOperator::Power, ..)
                        | Expression::BinaryOperator(_, BinaryOperator::Mul, ..)
                        | Expression::BinaryOperator(_, BinaryOperator::Add, ..)
                ) {
                    expr.pretty_print()
                } else {
                    with_parens_liberal(expr)
                }
            };

            add_parens_if_needed(lhs) + op.pretty_print() + add_parens_if_needed(rhs)
        }
        BinaryOperator::Sub => {
            let add_parens_if_needed = |expr: &Expression| {
                if matches!(
                    expr,
                    Expression::BinaryOperator(_, BinaryOperator::Power, ..)
                        | Expression::BinaryOperator(_, BinaryOperator::Mul, ..)
                ) {
                    expr.pretty_print()
                } else {
                    with_parens_liberal(expr)
                }
            };

            add_parens_if_needed(lhs) + op.pretty_print() + add_parens_if_needed(rhs)
        }
        BinaryOperator::Power if matches!(rhs, Expression::Scalar(_, n) if n.to_f64() == 2.0) => {
            with_parens(lhs) + m::operator("²")
        }
        BinaryOperator::Power if matches!(rhs, Expression::Scalar(_, n) if n.to_f64() == 3.0) => {
            with_parens(lhs) + m::operator("³")
        }
        _ => with_parens(lhs) + op.pretty_print() + with_parens(rhs),
    }
}

impl PrettyPrint for Expression {
    fn pretty_print(&self) -> Markup {
        use Expression::*;

        match self {
            Scalar(_, n) => pretty_scalar(*n),
            Identifier(_, name, _type) => m::identifier(name),
            UnitIdentifier(_, prefix, _name, full_name, _type) => {
                m::unit(format!("{}{}", prefix.as_string_long(), full_name))
            }
            UnaryOperator(_, self::UnaryOperator::Negate, expr, _type) => {
                m::operator("-") + with_parens(expr)
            }
            UnaryOperator(_, self::UnaryOperator::Factorial, expr, _type) => {
                with_parens(expr) + m::operator("!")
            }
            BinaryOperator(_, op, lhs, rhs, _type) => pretty_print_binop(op, lhs, rhs),
            FunctionCall(_, _, name, args, _type) => {
                m::identifier(name)
                    + m::operator("(")
                    + itertools::Itertools::intersperse(
                        args.iter().map(|e| e.pretty_print()),
                        m::operator(",") + m::space(),
                    )
                    .sum()
                    + m::operator(")")
            }
            Boolean(_, val) => val.pretty_print(),
            Condition(_, condition, then, else_) => {
                m::keyword("if")
                    + m::space()
                    + with_parens(&condition)
                    + m::space()
                    + m::keyword("then")
                    + m::space()
                    + with_parens(&then)
                    + m::space()
                    + m::keyword("else")
                    + m::space()
                    + with_parens(else_)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ReplaceSpans;
    use crate::markup::{Formatter, PlainTextFormatter};
    use crate::{prefix_parser::AcceptsPrefix, prefix_transformer::Transformer};

    fn parse(code: &str) -> Statement {
        let mut transformer = Transformer::new();
        transformer
            .register_name_and_aliases(
                &"meter".into(),
                &[
                    Decorator::Aliases(vec![("m".into(), Some(AcceptsPrefix::only_short()))]),
                    Decorator::MetricPrefixes,
                ],
                Span::dummy(),
            )
            .unwrap();
        transformer
            .register_name_and_aliases(
                &"second".into(),
                &[
                    Decorator::Aliases(vec![("s".into(), Some(AcceptsPrefix::only_short()))]),
                    Decorator::MetricPrefixes,
                ],
                Span::dummy(),
            )
            .unwrap();
        transformer
            .register_name_and_aliases(
                &"radian".into(),
                &[
                    Decorator::Aliases(vec![("rad".into(), Some(AcceptsPrefix::only_short()))]),
                    Decorator::MetricPrefixes,
                ],
                Span::dummy(),
            )
            .unwrap();
        transformer
            .register_name_and_aliases(
                &"degree".into(),
                &[Decorator::Aliases(vec![("°".into(), None)])],
                Span::dummy(),
            )
            .unwrap();
        transformer
            .register_name_and_aliases(
                &"inch".into(),
                &[Decorator::Aliases(vec![("in".into(), None)])],
                Span::dummy(),
            )
            .unwrap();

        let statements = crate::parser::parse(code, 0).unwrap();
        let transformed_statements = transformer.transform(statements).unwrap().replace_spans();
        crate::typechecker::TypeChecker::default()
            .check_statements(transformed_statements)
            .unwrap()[0]
            .clone()
    }

    fn pretty_print(stmt: &Statement) -> String {
        let markup = stmt.pretty_print();

        (PlainTextFormatter {}).format(&markup, false)
    }

    fn equal_pretty(input: &str, expected: &str) {
        let actual = pretty_print(&parse(input));
        println!("actual: '{actual}', expected: '{expected}'");
        assert_eq!(actual, expected);
    }

    #[test]
    fn pretty_print_basic() {
        equal_pretty("2+3", "2 + 3");
        equal_pretty("2*3", "2 × 3");
        equal_pretty("2^3", "2³");
        equal_pretty("2km", "2 kilometer");
        equal_pretty("2kilometer", "2 kilometer");
        equal_pretty("sin(30°)", "sin(30 degree)");
        equal_pretty("2*3*4", "2 × 3 × 4");
        equal_pretty("2*(3*4)", "2 × 3 × 4");
        equal_pretty("2+3+4", "2 + 3 + 4");
        equal_pretty("2+(3+4)", "2 + 3 + 4");
        equal_pretty("atan(30cm / 2m)", "atan(30 centimeter / 2 meter)");
        equal_pretty("1mrad -> °", "1 milliradian ➞ degree");
        equal_pretty("2km+2cm -> in", "2 kilometer + 2 centimeter ➞ inch");
        equal_pretty("2^3 + 4^5", "2³ + 4^5");
        equal_pretty("2^3 - 4^5", "2³ - 4^5");
        equal_pretty("2^3 * 4^5", "2³ × 4^5");
        equal_pretty("2 * 3 + 4 * 5", "2 × 3 + 4 × 5");
        equal_pretty("2 * 3 / 4", "2 × 3 / 4");
        equal_pretty("123.123 km² / s²", "123.123 × kilometer² / second²");
        equal_pretty(" sin(  2  ,  3  ,  4   )  ", "sin(2, 3, 4)");
    }

    fn roundtrip_check(code: &str) {
        let ast1 = parse(code);
        let ast2 = parse(&pretty_print(&ast1));
        assert_eq!(ast1, ast2);
    }

    #[test]
    fn pretty_print_roundtrip_check() {
        roundtrip_check("1.0");
        roundtrip_check("2");
        roundtrip_check("1 + 2");

        roundtrip_check("-2.3e-12387");
        roundtrip_check("2.3e-12387");
        roundtrip_check("18379173");
        roundtrip_check("2+3");
        roundtrip_check("2+3*5");
        roundtrip_check("-3^4+2/(4+2*3)");
        roundtrip_check("1-2-3-4-(5-6-7)");
        roundtrip_check("1/2/3/4/(5/6/7)");
        roundtrip_check("kg");
        roundtrip_check("2meter/second");
        roundtrip_check("a+b*c^d-e*f");
        roundtrip_check("sin(x)^3");
        roundtrip_check("sin(cos(atanh(x)+2))^3");
        roundtrip_check("2^3^4^5");
        roundtrip_check("(2^3)^(4^5)");
        roundtrip_check("sqrt(1.4^2 + 1.5^2) * cos(pi/3)^2");
        roundtrip_check("40 kilometer * 9.8meter/second^2 * 150centimeter");
        roundtrip_check("4/3 * pi * r³");
        roundtrip_check("vol * density -> kg");
        roundtrip_check("atan(30 centimeter / 2 meter)");
        roundtrip_check("500kilometer/second -> centimeter/second");
        roundtrip_check("länge * x_2 * µ * _prefixed");
        roundtrip_check("2meter^3");
        roundtrip_check("(2meter)^3");
        roundtrip_check("-sqrt(-30meter^3)");
        roundtrip_check("-3^4");
        roundtrip_check("(-3)^4");
        roundtrip_check("sin(2,3,4)");
        roundtrip_check("2^3!");
        roundtrip_check("-3!");
        roundtrip_check("(-3)!");
    }
}
