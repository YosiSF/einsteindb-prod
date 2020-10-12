// Copyright 2016 WHTCORPS INC
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

use std; // To refer to std::result::Result.

use embedded_promises::{
    ValueType,
    ValueTypeSet,
};

use edbn::parse::{
    ParseError,
};

use edbn::causetq::{
    PlainSymbol,
};

pub type Result<T> = std::result::Result<T, ParityFilterError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BindingError {
    NoBoundVariable,
    UnexpectedBinding,
    RepeatedBoundVariable, // TODO: include repeated variable(s).

    /// Expected `[[?x ?y]]` but got some other type of binding.  EinsteinDB is deliberately more strict
    /// than Datomic: we won't try to make sense of non-obvious (and potentially erroneous) bindings.
    ExpectedBindRel,

    /// Expected `[[?x ?y]]` or `[?x ...]` but got some other type of binding.  EinsteinDB is
    /// deliberately more strict than Datomic: we won't try to make sense of non-obvious (and
    /// potentially erroneous) bindings.
    ExpectedBindRelOrBindColl,

    /// Expected `[?x1 … ?xN]` or `[[?x1 … ?xN]]` but got some other number of bindings.  EinsteinDB is
    /// deliberately more strict than Datomic: we prefer placeholders to omission.
    InvalidNumberOfBindings { number: usize, expected: usize },
}

#[derive(Clone, Debug, Eq, Fail, PartialEq)]
pub enum ParityFilterError {
    #[fail(display = "{} var {} is duplicated", _0, _1)]
    DuplicateVariableError(PlainSymbol, &'static str),

    #[fail(display = "unexpected FnArg")]
    UnsupportedArgument,

    #[fail(display = "value of type {} provided for var {}, expected {}", _0, _1, _2)]
    InputTypeDisagreement(PlainSymbol, ValueType, ValueType),

    #[fail(display = "invalid number of arguments to {}: expected {}, got {}.", _0, _1, _2)]
    InvalidNumberOfArguments(PlainSymbol, usize, usize),

    #[fail(display = "invalid argument to {}: expected {} in position {}.", _0, _1, _2)]
    InvalidArgument(PlainSymbol, &'static str, usize),

    #[fail(display = "invalid argument to {}: expected one of {:?} in position {}.", _0, _1, _2)]
    InvalidArgumentType(PlainSymbol, ValueTypeSet, usize),

    // TODO: flesh this out.
    #[fail(display = "invalid expression in ground constant")]
    InvalidGroundConstant,

    #[fail(display = "invalid limit {} of type {}: expected natural number.", _0, _1)]
    InvalidLimit(String, ValueType),

    #[fail(display = "mismatched bindings in ground")]
    GroundBindingsMismatch,

    #[fail(display = "no solitonId found for causetid: {}", _0)]
    UnrecognizedCausetId(String),

    #[fail(display = "no function named {}", _0)]
    UnknownFunction(PlainSymbol),

    #[fail(display = ":limit var {} not present in :in", _0)]
    UnknownLimitVar(PlainSymbol),

    #[fail(display = "unbound variable {} in order clause or function call", _0)]
    UnboundVariable(PlainSymbol),

    // TODO: flesh out.
    #[fail(display = "non-matching variables in 'or' clause")]
    NonMatchingVariablesInOrClause,

    #[fail(display = "non-matching variables in 'not' clause")]
    NonMatchingVariablesInNotClause,

    #[fail(display = "binding error in {}: {:?}", _0, _1)]
    InvalidBinding(PlainSymbol, BindingError),

    #[fail(display = "{}", _0)]
    EdnParseError(#[cause] ParseError),
}

impl From<ParseError> for ParityFilterError {
    fn from(error: ParseError) -> ParityFilterError {
        ParityFilterError::EdnParseError(error)
    }
}
