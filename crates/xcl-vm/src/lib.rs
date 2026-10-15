pub mod ast;
pub mod bytecode;
pub mod lexer;
pub mod parser;
pub mod span;
pub mod value;
pub mod vm;

pub use bytecode::{Bytecode, Instr};
pub use value::{Value, FuncValue};
pub use vm::VM;
pub use span::Span;

pub fn evaluate(source: &str, vm: &mut VM) -> Value {
    let tokens = lexer::lex(source, "<input>");
    let ast = parser::parse(&tokens, "<input>");
    let bytecode = bytecode::compile(&ast);
    vm.run(&bytecode)
}