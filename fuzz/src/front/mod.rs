// The `.qx` lexer and parser on their own; `#[path]` resolves from this directory.

#[path = "../../../src/lang/ast.rs"]
pub mod ast;
#[path = "../../../src/lang/diag.rs"]
pub mod diag;
#[path = "../../../src/lang/lexer.rs"]
pub mod lexer;
#[path = "../../../src/lang/msg.rs"]
pub mod msg;
#[path = "../../../src/lang/parser.rs"]
pub mod parser;
