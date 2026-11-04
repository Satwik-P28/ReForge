use serde::{Deserialize, Serialize};

/// Constrained expression AST. Intentionally small.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Expr {
    LitNull,
    LitBool {
        value: bool,
    },
    LitInt {
        value: i64,
    },
    LitFloat {
        value: f64,
    },
    LitString {
        value: String,
    },
    /// Reference: root is state|input|output|var, path is field path.
    Ref {
        root: String,
        path: Vec<String>,
    },
    Not {
        arg: Box<Expr>,
    },
    And {
        args: Vec<Expr>,
    },
    Or {
        args: Vec<Expr>,
    },
    Eq {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Ne {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Lt {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Le {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Gt {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Ge {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    In {
        item: Box<Expr>,
        collection: Box<Expr>,
    },
    Contains {
        collection: Box<Expr>,
        item: Box<Expr>,
    },
    Len {
        arg: Box<Expr>,
    },
    IsNull {
        arg: Box<Expr>,
    },
    IsDefined {
        root: String,
        path: Vec<String>,
    },
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        else_: Box<Expr>,
    },
    Add {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Sub {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// Construct a map literal from evaluated entries (static keys).
    MapLit {
        entries: Vec<(String, Expr)>,
    },
    ListLit {
        items: Vec<Expr>,
    },
    /// Functional map update: returns a new map with `key` set to `value`.
    MapPut {
        map: Box<Expr>,
        key: Box<Expr>,
        value: Box<Expr>,
    },
    /// Functional map delete: returns a new map without `key`.
    MapRemove {
        map: Box<Expr>,
        key: Box<Expr>,
    },
    /// Read map entry by dynamic key. Missing key yields null.
    MapGet {
        map: Box<Expr>,
        key: Box<Expr>,
    },
    /// List of map keys as strings (insertion order).
    Keys {
        map: Box<Expr>,
    },
    /// Append one value to a list (returns new list).
    ListAppend {
        list: Box<Expr>,
        item: Box<Expr>,
    },
}
