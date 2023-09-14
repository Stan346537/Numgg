use crate::markup as m;
use crate::span::Span;
use crate::{
    arithmetic::Exponent, decorator::Decorator, markup::Markup, number::Number, prefix::Prefix,
    pretty_print::PrettyPrint, resolver::ModulePath,
};
use num_traits::Signed;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Factorial,
    Negate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    Add,
    Sub,
    Mul,
    Div,
    Power,
    ConvertTo,
    LessThan,
    GreaterThan,
    LessOrEqual,
    GreaterOrEqual,
    Equal,
    NotEqual,
}

impl PrettyPrint for BinaryOperator {
    fn pretty_print(&self) -> Markup {
        use BinaryOperator::*;

        match self {
            Add => m::space() + m::operator("+") + m::space(),
            Sub => m::space() + m::operator("-") + m::space(),
            Mul => m::space() + m::operator("×") + m::space(),
            Div => m::space() + m::operator("/") + m::space(),
            Power => m::operator("^"),
            ConvertTo => m::space() + m::operator("➞") + m::space(),
            LessThan => m::space() + m::operator("<") + m::space(),
            GreaterThan => m::space() + m::operator(">") + m::space(),
            LessOrEqual => m::space() + m::operator("≤") + m::space(),
            GreaterOrEqual => m::space() + m::operator("≥") + m::space(),
            Equal => m::space() + m::operator("==") + m::space(),
            NotEqual => m::space() + m::operator("≠") + m::space(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Scalar(Span, Number),
    Identifier(Span, String),
    UnitIdentifier(Span, Prefix, String, String),
    UnaryOperator {
        op: UnaryOperator,
        expr: Box<Expression>,
        span_op: Span,
    },
    BinaryOperator {
        op: BinaryOperator,
        lhs: Box<Expression>,
        rhs: Box<Expression>,
        span_op: Option<Span>, // not available for implicit multiplication and unicode exponents
    },
    FunctionCall(Span, Span, String, Vec<Expression>),

    Boolean(Span, bool),
    Condition(Span, Box<Expression>, Box<Expression>, Box<Expression>),
}

impl Expression {
    pub fn full_span(&self) -> Span {
        match self {
            Expression::Scalar(span, _) => *span,
            Expression::Identifier(span, _) => *span,
            Expression::UnitIdentifier(span, _, _, _) => *span,
            Expression::UnaryOperator {
                op: _,
                expr,
                span_op,
            } => span_op.extend(&expr.full_span()),
            Expression::BinaryOperator {
                op: _,
                lhs,
                rhs,
                span_op,
            } => {
                let mut span = lhs.full_span().extend(&rhs.full_span());
                if let Some(span_op) = span_op {
                    span = span.extend(span_op);
                }
                span
            }
            Expression::FunctionCall(_identifier_span, full_span, _, _) => *full_span,
            Expression::Boolean(span, _) => *span,
            Expression::Condition(span_if, _, _, then_expr) => {
                span_if.extend(&then_expr.full_span())
            }
        }
    }
}

#[cfg(test)]
macro_rules! scalar {
    ( $num:expr ) => {{
        crate::ast::Expression::Scalar(Span::dummy(), Number::from_f64($num))
    }};
}

#[cfg(test)]
macro_rules! identifier {
    ( $name:expr ) => {{
        crate::ast::Expression::Identifier(Span::dummy(), $name.into())
    }};
}

#[cfg(test)]
macro_rules! negate {
    ( $rhs:expr ) => {{
        crate::ast::Expression::UnaryOperator {
            op: UnaryOperator::Negate,
            expr: Box::new($rhs),
            span_op: Span::dummy(),
        }
    }};
}

#[cfg(test)]
macro_rules! factorial {
    ( $lhs:expr ) => {{
        crate::ast::Expression::UnaryOperator {
            op: UnaryOperator::Factorial,
            expr: Box::new($lhs),
            span_op: Span::dummy(),
        }
    }};
}

#[cfg(test)]
macro_rules! binop {
    ( $lhs:expr, $op:ident, $rhs: expr ) => {{
        crate::ast::Expression::BinaryOperator {
            op: BinaryOperator::$op,
            lhs: Box::new($lhs),
            rhs: Box::new($rhs),
            span_op: Some(Span::dummy()),
        }
    }};
}

#[cfg(test)]
macro_rules! conditional {
    ( $cond:expr, $lhs:expr, $rhs: expr ) => {{
        crate::ast::Expression::Condition(
            Span::dummy(),
            Box::new($cond),
            Box::new($lhs),
            Box::new($rhs),
        )
    }};
}

#[cfg(test)]
pub(crate) use binop;
#[cfg(test)]
pub(crate) use conditional;
#[cfg(test)]
pub(crate) use factorial;
#[cfg(test)]
pub(crate) use identifier;
#[cfg(test)]
pub(crate) use negate;
#[cfg(test)]
pub(crate) use scalar;

#[derive(Debug, Clone, PartialEq)]

pub enum DimensionExpression {
    Unity(Span),
    Dimension(Span, String),
    Multiply(Span, Box<DimensionExpression>, Box<DimensionExpression>),
    Divide(Span, Box<DimensionExpression>, Box<DimensionExpression>),
    Power(Span, Box<DimensionExpression>, Span, Exponent),
}

impl DimensionExpression {
    pub fn full_span(&self) -> Span {
        match self {
            DimensionExpression::Unity(s) => *s,
            DimensionExpression::Dimension(s, _) => *s,
            DimensionExpression::Multiply(span_op, lhs, rhs) => {
                span_op.extend(&lhs.full_span()).extend(&rhs.full_span())
            }
            DimensionExpression::Divide(span_op, lhs, rhs) => {
                span_op.extend(&lhs.full_span()).extend(&rhs.full_span())
            }
            DimensionExpression::Power(span_op, lhs, span_exponent, _exp) => {
                span_op.extend(&lhs.full_span()).extend(span_exponent)
            }
        }
    }
}

impl PrettyPrint for DimensionExpression {
    fn pretty_print(&self) -> Markup {
        match self {
            DimensionExpression::Unity(_) => m::type_identifier("1"),
            DimensionExpression::Dimension(_, ident) => m::type_identifier(ident),
            DimensionExpression::Multiply(_, lhs, rhs) => {
                lhs.pretty_print() + m::space() + m::operator("×") + m::space() + rhs.pretty_print()
            }
            DimensionExpression::Divide(_, lhs, rhs) => {
                lhs.pretty_print() + m::space() + m::operator("/") + m::space() + rhs.pretty_print()
            }
            DimensionExpression::Power(_, lhs, _, exp) => {
                m::operator("(")
                    + lhs.pretty_print()
                    + m::operator(")")
                    + m::operator("^")
                    + if exp.is_positive() {
                        m::value(format!("{exp}"))
                    } else {
                        m::operator("(") + m::value(format!("{exp}")) + m::operator(")")
                    }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProcedureKind {
    Print,
    AssertEq,
    Type,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Expression(Expression),
    DefineVariable {
        identifier_span: Span,
        identifier: String,
        expr: Expression,
        type_annotation: Option<DimensionExpression>,
    },
    DefineFunction {
        function_name_span: Span,
        function_name: String,
        type_parameters: Vec<(Span, String)>,
        /// Parameters, optionally with type annotations. The boolean argument specifies whether or not the parameter is variadic
        parameters: Vec<(Span, String, Option<DimensionExpression>, bool)>,
        /// Function body. If it is absent, the function is implemented via FFI
        body: Option<Expression>,
        return_type_span: Option<Span>,
        /// Optional annotated return type
        return_type_annotation: Option<DimensionExpression>,
    },
    DefineDimension(String, Vec<DimensionExpression>),
    DefineBaseUnit(Span, String, Option<DimensionExpression>, Vec<Decorator>),
    DefineDerivedUnit {
        identifier_span: Span,
        identifier: String,
        expr: Expression,
        type_annotation_span: Option<Span>,
        type_annotation: Option<DimensionExpression>,
        decorators: Vec<Decorator>,
    },
    ProcedureCall(Span, ProcedureKind, Vec<Expression>),
    ModuleImport(Span, ModulePath),
}

#[cfg(test)]
pub trait ReplaceSpans {
    fn replace_spans(&self) -> Self;
}

#[cfg(test)]
impl ReplaceSpans for DimensionExpression {
    fn replace_spans(&self) -> Self {
        match self {
            DimensionExpression::Unity(_) => DimensionExpression::Unity(Span::dummy()),
            DimensionExpression::Dimension(_, d) => {
                DimensionExpression::Dimension(Span::dummy(), d.clone())
            }
            DimensionExpression::Multiply(_, lhs, rhs) => DimensionExpression::Multiply(
                Span::dummy(),
                Box::new(lhs.replace_spans()),
                Box::new(rhs.replace_spans()),
            ),
            DimensionExpression::Divide(_, lhs, rhs) => DimensionExpression::Divide(
                Span::dummy(),
                Box::new(lhs.replace_spans()),
                Box::new(rhs.replace_spans()),
            ),
            DimensionExpression::Power(_, lhs, _, exp) => DimensionExpression::Power(
                Span::dummy(),
                Box::new(lhs.replace_spans()),
                Span::dummy(),
                *exp,
            ),
        }
    }
}

#[cfg(test)]
impl ReplaceSpans for Expression {
    fn replace_spans(&self) -> Self {
        match self {
            Expression::Scalar(_, name) => Expression::Scalar(Span::dummy(), *name),
            Expression::Identifier(_, name) => Expression::Identifier(Span::dummy(), name.clone()),
            Expression::UnitIdentifier(_, prefix, name, full_name) => {
                Expression::UnitIdentifier(Span::dummy(), *prefix, name.clone(), full_name.clone())
            }
            Expression::UnaryOperator {
                op,
                expr,
                span_op: _,
            } => Expression::UnaryOperator {
                op: *op,
                expr: Box::new(expr.replace_spans()),
                span_op: Span::dummy(),
            },
            Expression::BinaryOperator {
                op,
                lhs,
                rhs,
                span_op: _,
            } => Expression::BinaryOperator {
                op: *op,
                lhs: Box::new(lhs.replace_spans()),
                rhs: Box::new(rhs.replace_spans()),
                span_op: Some(Span::dummy()),
            },
            Expression::FunctionCall(_, _, name, args) => Expression::FunctionCall(
                Span::dummy(),
                Span::dummy(),
                name.clone(),
                args.iter().map(|a| a.replace_spans()).collect(),
            ),
            Expression::Boolean(_, val) => Expression::Boolean(Span::dummy(), *val),
            Expression::Condition(_, condition, then, else_) => Expression::Condition(
                Span::dummy(),
                Box::new(condition.replace_spans()),
                Box::new(then.replace_spans()),
                Box::new(else_.replace_spans()),
            ),
        }
    }
}

#[cfg(test)]
impl ReplaceSpans for Statement {
    fn replace_spans(&self) -> Self {
        match self {
            Statement::Expression(expr) => Statement::Expression(expr.replace_spans()),
            Statement::DefineVariable {
                identifier_span: _,
                identifier,
                expr,
                type_annotation,
            } => Statement::DefineVariable {
                identifier_span: Span::dummy(),
                identifier: identifier.clone(),
                expr: expr.replace_spans(),
                type_annotation: type_annotation.as_ref().map(|t| t.replace_spans()),
            },
            Statement::DefineFunction {
                function_name_span: _,
                function_name,
                type_parameters,
                parameters,
                body,
                return_type_span,
                return_type_annotation,
            } => Statement::DefineFunction {
                function_name_span: Span::dummy(),
                function_name: function_name.clone(),
                type_parameters: type_parameters
                    .iter()
                    .map(|(_, name)| (Span::dummy(), name.clone()))
                    .collect(),
                parameters: parameters
                    .iter()
                    .map(|(_, name, type_, is_variadic)| {
                        (
                            Span::dummy(),
                            name.clone(),
                            type_.as_ref().map(|t| t.replace_spans()),
                            *is_variadic,
                        )
                    })
                    .collect(),
                body: body.clone().map(|b| b.replace_spans()),
                return_type_span: return_type_span.map(|_| Span::dummy()),
                return_type_annotation: return_type_annotation.as_ref().map(|t| t.replace_spans()),
            },
            Statement::DefineDimension(name, dexprs) => Statement::DefineDimension(
                name.clone(),
                dexprs.iter().map(|t| t.replace_spans()).collect(),
            ),
            Statement::DefineBaseUnit(_, name, type_, decorators) => Statement::DefineBaseUnit(
                Span::dummy(),
                name.clone(),
                type_.as_ref().map(|t| t.replace_spans()),
                decorators.clone(),
            ),
            Statement::DefineDerivedUnit {
                identifier_span: _,
                identifier,
                expr,
                type_annotation_span,
                type_annotation,
                decorators,
            } => Statement::DefineDerivedUnit {
                identifier_span: Span::dummy(),
                identifier: identifier.clone(),
                expr: expr.replace_spans(),
                type_annotation_span: type_annotation_span.map(|_| Span::dummy()),
                type_annotation: type_annotation.as_ref().map(|t| t.replace_spans()),
                decorators: decorators.clone(),
            },
            Statement::ProcedureCall(_, proc, args) => Statement::ProcedureCall(
                Span::dummy(),
                proc.clone(),
                args.iter().map(|a| a.replace_spans()).collect(),
            ),
            Statement::ModuleImport(_, module_path) => {
                Statement::ModuleImport(Span::dummy(), module_path.clone())
            }
        }
    }
}

#[cfg(test)]
impl ReplaceSpans for Vec<Statement> {
    fn replace_spans(&self) -> Self {
        self.iter().map(|s| s.replace_spans()).collect()
    }
}
