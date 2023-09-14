use std::collections::HashMap;

use crate::ast::ProcedureKind;
use crate::interpreter::{
    Interpreter, InterpreterResult, InterpreterSettings, Result, RuntimeError,
};
use crate::prefix::Prefix;
use crate::typed_ast::{BinaryOperator, Expression, Statement, UnaryOperator};
use crate::unit::Unit;
use crate::unit_registry::UnitRegistry;
use crate::vm::{Constant, ExecutionContext, Op, Vm};
use crate::{decorator, ffi};

pub struct BytecodeInterpreter {
    vm: Vm,
    /// List of local variables currently in scope
    local_variables: Vec<String>,
    // Maps names of units to indices of the respective constants in the VM
    unit_name_to_constant_index: HashMap<String, u16>,
}

impl BytecodeInterpreter {
    fn compile_expression(&mut self, expr: &Expression) -> Result<()> {
        match expr {
            Expression::Scalar(_span, n) => {
                let index = self.vm.add_constant(Constant::Scalar(n.to_f64()));
                self.vm.add_op1(Op::LoadConstant, index);
            }
            Expression::Identifier(_span, identifier, _type) => {
                if let Some(position) = self.local_variables.iter().position(|n| n == identifier) {
                    self.vm.add_op1(Op::GetLocal, position as u16); // TODO: check overflow
                } else {
                    let identifier_idx = self.vm.add_global_identifier(identifier, None);
                    self.vm.add_op1(Op::GetVariable, identifier_idx);
                }
            }
            Expression::UnitIdentifier(_span, prefix, unit_name, _full_name, _type) => {
                let index = self
                    .unit_name_to_constant_index
                    .get(unit_name)
                    .expect("unit should already exist");

                self.vm.add_op1(Op::LoadConstant, *index);

                if prefix != &Prefix::none() {
                    let prefix_idx = self.vm.add_prefix(*prefix);
                    self.vm.add_op1(Op::ApplyPrefix, prefix_idx);
                }
            }
            Expression::UnaryOperator(_span, UnaryOperator::Negate, rhs, _type) => {
                self.compile_expression(rhs)?;
                self.vm.add_op(Op::Negate);
            }
            Expression::UnaryOperator(_span, UnaryOperator::Factorial, lhs, _type) => {
                self.compile_expression(lhs)?;
                self.vm.add_op(Op::Factorial);
            }
            Expression::BinaryOperator(_span, operator, lhs, rhs, _type) => {
                self.compile_expression(lhs)?;
                self.compile_expression(rhs)?;

                let op = match operator {
                    BinaryOperator::Add => Op::Add,
                    BinaryOperator::Sub => Op::Subtract,
                    BinaryOperator::Mul => Op::Multiply,
                    BinaryOperator::Div => Op::Divide,
                    BinaryOperator::Power => Op::Power,
                    BinaryOperator::ConvertTo => Op::ConvertTo,
                    BinaryOperator::LessThan => Op::LessThan,
                    BinaryOperator::GreaterThan => Op::GreaterThan,
                    BinaryOperator::LessOrEqual => Op::LessOrEqual,
                    BinaryOperator::GreaterOrEqual => Op::GreatorOrEqual,
                    BinaryOperator::Equal => Op::Equal,
                    BinaryOperator::NotEqual => Op::NotEqual,
                };
                self.vm.add_op(op);
            }
            Expression::FunctionCall(_span, _full_span, name, args, _type) => {
                // Put all arguments on top of the stack
                for arg in args {
                    self.compile_expression(arg)?;
                }

                if let Some(idx) = self.vm.get_ffi_callable_idx(name) {
                    // TODO: check overflow:
                    self.vm.add_op2(Op::FFICallFunction, idx, args.len() as u16);
                } else {
                    let idx = self.vm.get_function_idx(name);

                    self.vm.add_op2(Op::Call, idx, args.len() as u16); // TODO: check overflow
                }
            }
            Expression::Boolean(_, val) => {
                let index = self.vm.add_constant(Constant::Boolean(*val));
                self.vm.add_op1(Op::LoadConstant, index);
            }
            Expression::Condition(_, condition, then_expr, else_expr) => {
                self.compile_expression(condition)?;

                let if_jump_offset = self.vm.current_offset() + 1; // +1 for the opcode
                self.vm.add_op1(Op::JumpIfFalse, 0xffff);

                self.compile_expression(then_expr)?;

                let else_jump_offset = self.vm.current_offset() + 1;
                self.vm.add_op1(Op::Jump, 0xffff);

                let else_block_offset = self.vm.current_offset();
                self.vm
                    .patch_u16_value_at(if_jump_offset, else_block_offset - (if_jump_offset + 2));

                self.compile_expression(else_expr)?;

                let end_offset = self.vm.current_offset();

                self.vm
                    .patch_u16_value_at(else_jump_offset, end_offset - (else_jump_offset + 2));
            }
        };

        Ok(())
    }

    fn compile_expression_with_simplify(&mut self, expr: &Expression) -> Result<()> {
        self.compile_expression(expr)?;

        match expr {
            Expression::Scalar(..)
            | Expression::Identifier(..)
            | Expression::UnitIdentifier(..)
            | Expression::FunctionCall(..)
            | Expression::UnaryOperator(..)
            | Expression::BinaryOperator(_, BinaryOperator::ConvertTo, _, _, _)
            | Expression::Boolean(..)
            | Expression::Condition(..) => {}
            Expression::BinaryOperator(..) => {
                self.vm.add_op(Op::FullSimplify);
            }
        }

        Ok(())
    }

    fn compile_statement(&mut self, stmt: &Statement) -> Result<()> {
        match stmt {
            Statement::Expression(expr) => {
                self.compile_expression_with_simplify(expr)?;
                self.vm.add_op(Op::Return);
            }
            Statement::DefineVariable(identifier, expr, _type_annotation, _type) => {
                self.compile_expression_with_simplify(expr)?;
                let identifier_idx = self.vm.add_global_identifier(identifier, None);
                self.vm.add_op1(Op::SetVariable, identifier_idx);
            }
            Statement::DefineFunction(
                name,
                _type_parameters,
                parameters,
                Some(expr),
                _return_type_annotation,
                _return_type,
            ) => {
                self.vm.begin_function(name);
                for parameter in parameters.iter() {
                    self.local_variables.push(parameter.1.clone());
                }
                self.compile_expression_with_simplify(expr)?;
                self.vm.add_op(Op::Return);
                for _ in parameters {
                    self.local_variables.pop();
                }
                self.vm.end_function();
            }
            Statement::DefineFunction(
                name,
                _type_parameters,
                parameters,
                None,
                _return_type_annotation,
                _return_type,
            ) => {
                // Declaring a foreign function does not generate any bytecode. But we register
                // its name and arity here to be able to distinguish it from normal functions.

                let is_variadic = parameters.iter().any(|p| p.2);

                self.vm.add_foreign_function(
                    name,
                    if is_variadic {
                        1..=usize::MAX
                    } else {
                        parameters.len()..=parameters.len()
                    },
                );
            }
            Statement::DefineDimension(_name, _dexprs) => {
                // Declaring a dimension is like introducing a new type. The information
                // is only relevant for the type checker. Nothing happens at run time.
            }
            Statement::DefineBaseUnit(unit_name, decorators, _type_annotation, type_) => {
                self.vm
                    .unit_registry
                    .add_base_unit(unit_name, type_.clone())
                    .map_err(RuntimeError::UnitRegistryError)?;

                let constant_idx = self.vm.add_constant(Constant::Unit(Unit::new_base(
                    unit_name,
                    &crate::decorator::get_canonical_unit_name(unit_name.as_str(), &decorators[..]),
                )));
                for (name, _) in decorator::name_and_aliases(unit_name, decorators) {
                    self.unit_name_to_constant_index
                        .insert(name.into(), constant_idx);
                }
            }
            Statement::DefineDerivedUnit(unit_name, expr, decorators, _type_annotation) => {
                let constant_idx = self
                    .vm
                    .add_constant(Constant::Unit(Unit::new_base("<dummy>", "<dummy>"))); // TODO: dummy is just a temp. value until the SetUnitConstant op runs
                let identifier_idx = self.vm.add_global_identifier(
                    unit_name,
                    Some(&crate::decorator::get_canonical_unit_name(
                        unit_name.as_str(),
                        &decorators[..],
                    )),
                ); // TODO: there is some asymmetry here because we do not introduce identifiers for base units

                self.compile_expression_with_simplify(expr)?;
                self.vm
                    .add_op2(Op::SetUnitConstant, identifier_idx, constant_idx);

                // TODO: code duplication with DeclareBaseUnit branch above
                for (name, _) in decorator::name_and_aliases(unit_name, decorators) {
                    self.unit_name_to_constant_index
                        .insert(name.into(), constant_idx);
                }
            }
            Statement::ProcedureCall(ProcedureKind::Type, args) => {
                assert_eq!(args.len(), 1);
                let arg = &args[0];
                let type_str = format!("{}", arg.get_type());

                let idx = self.vm.add_string(type_str);
                self.vm.add_op1(Op::PrintString, idx);
            }
            Statement::ProcedureCall(kind, args) => {
                // Put all arguments on top of the stack
                for arg in args {
                    self.compile_expression_with_simplify(arg)?;
                }

                let name = &ffi::procedures().get(kind).unwrap().name;

                let idx = self.vm.get_ffi_callable_idx(name).unwrap();
                self.vm
                    .add_op2(Op::FFICallProcedure, idx, args.len() as u16); // TODO: check overflow
            }
        }

        Ok(())
    }

    fn run(&mut self, settings: &mut InterpreterSettings) -> Result<InterpreterResult> {
        let mut ctx = ExecutionContext {
            print_fn: &mut settings.print_fn,
        };

        self.vm.disassemble(&mut ctx);

        let result = self.vm.run(&mut ctx);

        self.vm.debug(&mut ctx);

        result
    }

    pub(crate) fn set_debug(&mut self, activate: bool) {
        self.vm.set_debug(activate);
    }
}

impl Interpreter for BytecodeInterpreter {
    fn new() -> Self {
        Self {
            vm: Vm::new(),
            local_variables: vec![],
            unit_name_to_constant_index: HashMap::new(),
        }
    }

    fn interpret_statements(
        &mut self,
        settings: &mut InterpreterSettings,
        statements: &[Statement],
    ) -> Result<InterpreterResult> {
        if statements.is_empty() {
            return Err(RuntimeError::NoStatements);
        };

        for statement in statements {
            self.compile_statement(statement)?;
        }

        self.run(settings)
    }

    fn get_unit_registry(&self) -> &UnitRegistry {
        &self.vm.unit_registry
    }
}
