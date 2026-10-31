//! Lexical scopes. Variables, functions and modules live in separate
//! namespaces; `$` variables are additionally looked up dynamically through
//! the evaluator's scope stack.

use std::cell::RefCell;
use std::rc::Rc;

use rustc_hash::FxHashMap;
use scad_syntax::ast::{FunctionDef, ModuleDef, Name};

use crate::value::Value;

/// A definition together with the id of the file it was written in.
pub type Defined<T> = (Rc<T>, u32);

#[derive(Default)]
pub struct Scope {
    pub parent: Option<Rc<Scope>>,
    vars: RefCell<FxHashMap<Name, Value>>,
    functions: RefCell<FxHashMap<Name, Defined<FunctionDef>>>,
    modules: RefCell<FxHashMap<Name, Defined<ModuleDef>>>,
    /// Libraries imported with `use <...>`: only their functions and
    /// modules are visible, resolved after the lexical chain.
    used: RefCell<Vec<Rc<Scope>>>,
}

impl Scope {
    pub fn root() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub fn child(parent: &Rc<Scope>) -> Rc<Self> {
        Rc::new(Self {
            parent: Some(parent.clone()),
            ..Self::default()
        })
    }

    pub fn set_var(&self, name: Name, value: Value) {
        self.vars.borrow_mut().insert(name, value);
    }

    pub fn local_var(&self, name: &str) -> Option<Value> {
        self.vars.borrow().get(name).cloned()
    }

    pub fn has_local_var(&self, name: &str) -> bool {
        self.vars.borrow().contains_key(name)
    }

    /// Lexical lookup through the parent chain.
    pub fn lookup_var(&self, name: &str) -> Option<Value> {
        let mut scope = self;
        loop {
            if let Some(value) = scope.local_var(name) {
                return Some(value);
            }
            scope = scope.parent.as_deref()?;
        }
    }

    pub fn define_function(&self, def: Rc<FunctionDef>, file: u32) {
        self.functions
            .borrow_mut()
            .insert(def.name.clone(), (def, file));
    }

    pub fn define_module(&self, def: Rc<ModuleDef>, file: u32) {
        self.modules
            .borrow_mut()
            .insert(def.name.clone(), (def, file));
    }

    pub fn add_used(&self, library: Rc<Scope>) {
        self.used.borrow_mut().push(library);
    }

    /// Finds a function and the scope that defines it.
    pub fn lookup_function(
        self: &Rc<Self>,
        name: &str,
    ) -> Option<(Defined<FunctionDef>, Rc<Scope>)> {
        self.lookup_definition(name, |s, n| s.functions.borrow().get(n).cloned())
    }

    pub fn lookup_module(self: &Rc<Self>, name: &str) -> Option<(Defined<ModuleDef>, Rc<Scope>)> {
        self.lookup_definition(name, |s, n| s.modules.borrow().get(n).cloned())
    }

    fn lookup_definition<T>(
        self: &Rc<Self>,
        name: &str,
        get: impl Fn(&Scope, &str) -> Option<T> + Copy,
    ) -> Option<(T, Rc<Scope>)> {
        self.lookup_definition_in(name, get, &mut Vec::new())
    }

    /// `visited` breaks cycles between libraries that `use` each other.
    fn lookup_definition_in<T>(
        self: &Rc<Self>,
        name: &str,
        get: impl Fn(&Scope, &str) -> Option<T> + Copy,
        visited: &mut Vec<*const Scope>,
    ) -> Option<(T, Rc<Scope>)> {
        let mut scope = self.clone();
        loop {
            if let Some(def) = get(&scope, name) {
                return Some((def, scope));
            }
            match scope.parent.clone() {
                Some(parent) => scope = parent,
                None => break,
            }
        }
        // Not found lexically: try the libraries imported with `use`,
        // nearest scope first.
        let mut scope = Some(self.clone());
        while let Some(current) = scope {
            for library in current.used.borrow().iter() {
                let id = Rc::as_ptr(library);
                if visited.contains(&id) {
                    continue;
                }
                visited.push(id);
                if let Some(found) = library.lookup_definition_in(name, get, visited) {
                    return Some(found);
                }
            }
            scope = current.parent.clone();
        }
        None
    }
}
