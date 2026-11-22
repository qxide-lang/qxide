// Which elaborator builds the circuit. Only the tree-walking interpreter exists so far.

use super::ast::Program;
use super::diag::Diagnostic;
use super::interp::{self, Elaborated};
use super::stdlib::Host;
use super::Limits;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    // The tree-walking interpreter over the checked AST (src/lang/interp.rs).
    Ast,
}

impl Engine {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn parse(s: &str) -> Option<Engine> {
        match s {
            "ast" => Some(Engine::Ast),
            _ => None,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn name(self) -> &'static str {
        match self {
            Engine::Ast => "ast",
        }
    }
}

/// Runs `main` once with `engine`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn elaborate(
    p: &Program,
    host: &mut dyn Host,
    limits: &Limits,
    engine: Engine,
) -> Result<Elaborated, Diagnostic> {
    match engine {
        Engine::Ast => interp::elaborate(p, host, limits),
    }
}
