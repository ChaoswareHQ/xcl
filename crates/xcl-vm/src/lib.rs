pub mod ast;
pub mod bytecode;
pub mod lexer;
pub mod parser;
pub mod span;
pub mod value;
pub mod vm;
pub mod native;
pub mod args;
pub mod wellknown;

pub use bytecode::{Bytecode, Instr};
pub use value::{
    Value, FuncValue, HandleKind, RawPtr,
    NativeEntry, NamedParam, NativeKind, BuiltinImpl,
    TypeDescriptor, TypeKind, TypeField, TypeVariant,
    DomainCtx, NativeArg, extract_arg,
};
pub use vm::{VM, DomainSlot};
pub use native::{NATIVE_REGISTRY, TYPE_REGISTRY};
pub use args::split_named_args;
pub use wellknown::WellKnown;
pub use span::Span;

pub fn evaluate(source: &str, vm: &mut VM) -> Value {
    let tokens = lexer::lex(source, "<input>");
    let ast = parser::parse(&tokens, source);
    let bytecode = bytecode::compile(&ast);
    vm.run(&bytecode)
}