//! OpenSCAD language front end: lexer, lossless syntax tree and AST.
//!
//! The rowan syntax tree preserves every character of the source (comments
//! and formatting included), which lets tools edit code surgically. The
//! evaluator works on the owned [`ast`], produced by [`Parse::tree`].

pub mod ast;
pub mod lexer;
mod line_index;
mod lower;
mod parser;
mod syntax_kind;

pub use line_index::LineIndex;
pub use parser::SyntaxError;
pub use rowan::{TextRange, TextSize};
pub use syntax_kind::{ScadLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

pub struct Parse {
    green: rowan::GreenNode,
    errors: Vec<SyntaxError>,
}

impl Parse {
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }

    pub fn tree(&self) -> ast::SourceFile {
        lower::lower_file(&self.syntax())
    }
}

pub fn parse(text: &str) -> Parse {
    let (green, errors) = parser::parse_text(text);
    Parse { green, errors }
}

#[cfg(test)]
mod tests;
