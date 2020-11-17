// Copyright 2020 WHTCORPS INC
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

///! This module defines some embedded types that support find expressions: sources,
///! variables, expressions, etc.
///! These are produced as 'fuel' by the causetq parser, consumed by the causetq
///! translator and executor.
///!
///! Many of these types are defined as simple structs that are little more than
///! a richer type alias: a variable, for example, is really just a fancy kind
///! of string.
///!
///! At some point in the future, we might consider reducing copying and memory
///! usage by recasting all of these string-holding structs and enums in terms
///! of string references, with those references being slices of some parsed
///! input causetq string, and valid for the lifetime of that string.
///!
///! For now, for the sake of simplicity, all of these strings are heap-allocated.
///!
///! Furthermore, we might cut out some of the chaff here: each time a 'tagged'
///! type is used within an enum, we have an opportunity to simplify and use the
///! inner type directly in conjunction with matching on the enum. Before diving
///! deeply into this it's worth recognizing that this loss of 'sovereignty' is
///! a tradeoff against well-typed function signatures and other such boundaries.

use std::collections::{
    BTreeSet,
    HashSet,
};

use std;
use std::fmt;
use std::rc::{
    Rc,
};

use ::{
    BigInt,
    DateTime,
    OrderedFloat,
    Uuid,
    Utc,
};

use ::value_rc::{
    FromRc,
    ValueRc,
};

pub use ::{
    Keyword,
    PlainSymbol,
};

pub type SrcVarName = String;          // Do not include the required syntactic '$'.

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Variable(pub Rc<PlainSymbol>);

impl Variable {
    pub fn as_str(&self) -> &str {
        self.0.as_ref().0.as_str()
    }

    pub fn to_string(&self) -> String {
        self.0.as_ref().0.clone()
    }

    pub fn name(&self) -> PlainSymbol {
        self.0.as_ref().clone()
    }

    /// Return a new `Variable`, assuming that the provided string is a valid name.
    pub fn from_valid_name(name: &str) -> Variable {
        let s = PlainSymbol::plain(name);
        assert!(s.is_var_symbol());
        Variable(Rc::new(s))
    }
}

pub trait FromValue<T> {
    fn from_value(v: &::ValueAndSpan) -> Option<T>;
}

/// If the provided EDBN value is a PlainSymbol beginning with '?', return
/// it wrapped in a Variable. If not, return None.

impl FromValue<Variable> for Variable {
    fn from_value(v: &::ValueAndSpan) -> Option<Variable> {
        if let ::SpannedValue::PlainSymbol(ref s) = v.inner {
            Variable::from_symbol(s)
        } else {
            None
        }
    }
}

impl Variable {
    pub fn from_rc(sym: Rc<PlainSymbol>) -> Option<Variable> {
        if sym.is_var_symbol() {
            Some(Variable(sym.clone()))
        } else {
            None
        }
    }

    /// TODO: intern strings. #398.
    pub fn from_symbol(sym: &PlainSymbol) -> Option<Variable> {
        if sym.is_var_symbol() {
            Some(Variable(Rc::new(sym.clone())))
        } else {
            None
        }
    }
}

impl fmt::Debug for Variable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "var({})", self.0)
    }
}

impl std::fmt::Display for Variable {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CausetQFunction(pub PlainSymbol);

impl FromValue<CausetQFunction> for CausetQFunction {
    fn from_value(v: &::ValueAndSpan) -> Option<CausetQFunction> {
        if let ::SpannedValue::PlainSymbol(ref s) = v.inner {
            CausetQFunction::from_symbol(s)
        } else {
            None
        }
    }
}

impl CausetQFunction {
    pub fn from_symbol(sym: &PlainSymbol) -> Option<CausetQFunction> {
        // TODO: validate the acceptable set of function names.
        Some(CausetQFunction(sym.clone()))
    }
}

impl std::fmt::Display for CausetQFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Direction {
    Ascending,
    Descending,
}

/// An abstract declaration of ordering: direction and variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Order(pub Direction, pub Variable);   // Future: Element instead of Variable?

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SrcVar {
    DefaultSrc,
    NamedSrc(SrcVarName),
}

impl FromValue<SrcVar> for SrcVar {
    fn from_value(v: &::ValueAndSpan) -> Option<SrcVar> {
        if let ::SpannedValue::PlainSymbol(ref s) = v.inner {
            SrcVar::from_symbol(s)
        } else {
            None
        }
    }
}

impl SrcVar {
    pub fn from_symbol(sym: &PlainSymbol) -> Option<SrcVar> {
        if sym.is_src_symbol() {
            if sym.0 == "$" {
                Some(SrcVar::DefaultSrc)
            } else {
                Some(SrcVar::NamedSrc(sym.name().to_string()))
            }
        } else {
            None
        }
    }
}

/// These are the scalar values representable in EDBN.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NonIntegerConstant {
    Boolean(bool),
    BigInteger(BigInt),
    Float(OrderedFloat<f64>),
    Text(ValueRc<String>),
    Instant(DateTime<Utc>),
    Uuid(Uuid),
}

impl<'a> From<&'a str> for NonIntegerConstant {
    fn from(val: &'a str) -> NonIntegerConstant {
        NonIntegerConstant::Text(ValueRc::new(val.to_string()))
    }
}

impl From<String> for NonIntegerConstant {
    fn from(val: String) -> NonIntegerConstant {
        NonIntegerConstant::Text(ValueRc::new(val))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StackedPerceptron {
    Variable(Variable),
    SrcVar(SrcVar),
    SolitonIdOrInteger(i64),
    CausetIdOrKeyword(Keyword),
    Constant(NonIntegerConstant),
    // The collection values representable in EDBN.  There's no advantage to destructuring up front,
    // since consumers will need to handle arbitrarily nested EDBN themselves anyway.
    Vector(Vec<StackedPerceptron>),
}

impl FromValue<StackedPerceptron> for StackedPerceptron {
    fn from_value(v: &::ValueAndSpan) -> Option<StackedPerceptron> {
        use ::SpannedValue::*;
        match v.inner {
            Integer(x) =>
                Some(StackedPerceptron::SolitonIdOrInteger(x)),
            PlainSymbol(ref x) if x.is_src_symbol() =>
                SrcVar::from_symbol(x).map(StackedPerceptron::SrcVar),
            PlainSymbol(ref x) if x.is_var_symbol() =>
                Variable::from_symbol(x).map(StackedPerceptron::Variable),
            PlainSymbol(_) => None,
            Keyword(ref x) =>
                Some(StackedPerceptron::CausetIdOrKeyword(x.clone())),
            Instant(x) =>
                Some(StackedPerceptron::Constant(NonIntegerConstant::Instant(x))),
            Uuid(x) =>
                Some(StackedPerceptron::Constant(NonIntegerConstant::Uuid(x))),
            Boolean(x) =>
                Some(StackedPerceptron::Constant(NonIntegerConstant::Boolean(x))),
            Float(x) =>
                Some(StackedPerceptron::Constant(NonIntegerConstant::Float(x))),
            BigInteger(ref x) =>
                Some(StackedPerceptron::Constant(NonIntegerConstant::BigInteger(x.clone()))),
            Text(ref x) =>
                // TODO: intern strings. #398.
                Some(StackedPerceptron::Constant(x.clone().into())),
            Nil |
            NamespacedSymbol(_) |
            Vector(_) |
            List(_) |
            Set(_) |
            Map(_) => None,
        }
    }
}

// For display in column headings in the repl.
impl std::fmt::Display for StackedPerceptron {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            &StackedPerceptron::Variable(ref var) => write!(f, "{}", var),
            &StackedPerceptron::SrcVar(ref var) => {
                if var == &SrcVar::DefaultSrc {
                    write!(f, "$")
                } else {
                    write!(f, "{:?}", var)
                }
            },
            &StackedPerceptron::SolitonIdOrInteger(solitonId) => write!(f, "{}", solitonId),
            &StackedPerceptron::CausetIdOrKeyword(ref kw) => write!(f, "{}", kw),
            &StackedPerceptron::Constant(ref constant) => write!(f, "{:?}", constant),
            &StackedPerceptron::Vector(ref vec) => write!(f, "{:?}", vec),
        }
    }
}

impl StackedPerceptron {
    pub fn as_variable(&self) -> Option<&Variable> {
        match self {
            &StackedPerceptron::Variable(ref v) => Some(v),
            _ => None,
        }
    }
}

/// e, a, causetx can't be values -- no strings, no floats -- and so
/// they can only be variables, instanton IDs, causetid keywords, or
/// placeholders.
/// This encoding allows us to represent integers that aren't
/// instanton IDs. That'll get filtered out in the context of the
/// database.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PatternNonValuePlace {
    Placeholder,
    Variable(Variable),
    SolitonId(i64),                       // Will always be +ve. See #190.
    CausetId(ValueRc<Keyword>),
}

impl From<Rc<Keyword>> for PatternNonValuePlace {
    fn from(value: Rc<Keyword>) -> Self {
        PatternNonValuePlace::CausetId(ValueRc::from_rc(value))
    }
}

impl From<Keyword> for PatternNonValuePlace {
    fn from(value: Keyword) -> Self {
        PatternNonValuePlace::CausetId(ValueRc::new(value))
    }
}

impl PatternNonValuePlace {
    // I think we'll want move variants, so let's leave these here for now.
    #[allow(dead_code)]
    fn into_pattern_value_place(self) -> PatternValuePlace {
        match self {
            PatternNonValuePlace::Placeholder => PatternValuePlace::Placeholder,
            PatternNonValuePlace::Variable(x) => PatternValuePlace::Variable(x),
            PatternNonValuePlace::SolitonId(x)    => PatternValuePlace::SolitonIdOrInteger(x),
            PatternNonValuePlace::CausetId(x)    => PatternValuePlace::CausetIdOrKeyword(x),
        }
    }

    fn to_pattern_value_place(&self) -> PatternValuePlace {
        match *self {
            PatternNonValuePlace::Placeholder     => PatternValuePlace::Placeholder,
            PatternNonValuePlace::Variable(ref x) => PatternValuePlace::Variable(x.clone()),
            PatternNonValuePlace::SolitonId(x)        => PatternValuePlace::SolitonIdOrInteger(x),
            PatternNonValuePlace::CausetId(ref x)    => PatternValuePlace::CausetIdOrKeyword(x.clone()),
        }
    }
}

impl FromValue<PatternNonValuePlace> for PatternNonValuePlace {
    fn from_value(v: &::ValueAndSpan) -> Option<PatternNonValuePlace> {
        match v.inner {
            ::SpannedValue::Integer(x) => if x >= 0 {
                Some(PatternNonValuePlace::SolitonId(x))
            } else {
                None
            },
            ::SpannedValue::PlainSymbol(ref x) => if x.0.as_str() == "_" {
                Some(PatternNonValuePlace::Placeholder)
            } else {
                if let Some(v) = Variable::from_symbol(x) {
                    Some(PatternNonValuePlace::Variable(v))
                } else {
                    None
                }
            },
            ::SpannedValue::Keyword(ref x) =>
                Some(x.clone().into()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CausetIdOrSolitonId {
    CausetId(Keyword),
    SolitonId(i64),
}

/// The `v` part of a pattern can be much broader: it can represent
/// integers that aren't instanton IDs (particularly negative integers),
/// strings, and all the rest. We group those under `Constant`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PatternValuePlace {
    Placeholder,
    Variable(Variable),
    SolitonIdOrInteger(i64),
    CausetIdOrKeyword(ValueRc<Keyword>),
    Constant(NonIntegerConstant),
}

impl From<Rc<Keyword>> for PatternValuePlace {
    fn from(value: Rc<Keyword>) -> Self {
        PatternValuePlace::CausetIdOrKeyword(ValueRc::from_rc(value))
    }
}

impl From<Keyword> for PatternValuePlace {
    fn from(value: Keyword) -> Self {
        PatternValuePlace::CausetIdOrKeyword(ValueRc::new(value))
    }
}

impl FromValue<PatternValuePlace> for PatternValuePlace {
    fn from_value(v: &::ValueAndSpan) -> Option<PatternValuePlace> {
        match v.inner {
            ::SpannedValue::Integer(x) =>
                Some(PatternValuePlace::SolitonIdOrInteger(x)),
            ::SpannedValue::PlainSymbol(ref x) if x.0.as_str() == "_" =>
                Some(PatternValuePlace::Placeholder),
            ::SpannedValue::PlainSymbol(ref x) =>
                Variable::from_symbol(x).map(PatternValuePlace::Variable),
            ::SpannedValue::Keyword(ref x) if x.is_namespaced() =>
                Some(x.clone().into()),
            ::SpannedValue::Boolean(x) =>
                Some(PatternValuePlace::Constant(NonIntegerConstant::Boolean(x))),
            ::SpannedValue::Float(x) =>
                Some(PatternValuePlace::Constant(NonIntegerConstant::Float(x))),
            ::SpannedValue::BigInteger(ref x) =>
                Some(PatternValuePlace::Constant(NonIntegerConstant::BigInteger(x.clone()))),
            ::SpannedValue::Instant(x) =>
                Some(PatternValuePlace::Constant(NonIntegerConstant::Instant(x))),
            ::SpannedValue::Text(ref x) =>
                // TODO: intern strings. #398.
                Some(PatternValuePlace::Constant(x.clone().into())),
            ::SpannedValue::Uuid(ref u) =>
                Some(PatternValuePlace::Constant(NonIntegerConstant::Uuid(u.clone()))),

            // These don't appear in queries.
            ::SpannedValue::Nil => None,
            ::SpannedValue::NamespacedSymbol(_) => None,
            ::SpannedValue::Keyword(_) => None,                // … yet.
            ::SpannedValue::Map(_) => None,
            ::SpannedValue::List(_) => None,
            ::SpannedValue::Set(_) => None,
            ::SpannedValue::Vector(_) => None,
        }
    }
}

impl PatternValuePlace {
    // I think we'll want move variants, so let's leave these here for now.
    #[allow(dead_code)]
    fn into_pattern_non_value_place(self) -> Option<PatternNonValuePlace> {
        match self {
            PatternValuePlace::Placeholder       => Some(PatternNonValuePlace::Placeholder),
            PatternValuePlace::Variable(x)       => Some(PatternNonValuePlace::Variable(x)),
            PatternValuePlace::SolitonIdOrInteger(x) => if x >= 0 {
                Some(PatternNonValuePlace::SolitonId(x))
            } else {
                None
            },
            PatternValuePlace::CausetIdOrKeyword(x) => Some(PatternNonValuePlace::CausetId(x)),
            PatternValuePlace::Constant(_)       => None,
        }
    }

    fn to_pattern_non_value_place(&self) -> Option<PatternNonValuePlace> {
        match *self {
            PatternValuePlace::Placeholder           => Some(PatternNonValuePlace::Placeholder),
            PatternValuePlace::Variable(ref x)       => Some(PatternNonValuePlace::Variable(x.clone())),
            PatternValuePlace::SolitonIdOrInteger(x)     => if x >= 0 {
                Some(PatternNonValuePlace::SolitonId(x))
            } else {
                None
            },
            PatternValuePlace::CausetIdOrKeyword(ref x) => Some(PatternNonValuePlace::CausetId(x.clone())),
            PatternValuePlace::Constant(_)           => None,
        }
    }
}

// Not yet used.
// pub enum PullDefaultValue {
//     SolitonIdOrInteger(i64),
//     CausetIdOrKeyword(Rc<Keyword>),
//     Constant(NonIntegerConstant),
// }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PullConcreteAttribute {
    CausetId(Rc<Keyword>),
    SolitonId(i64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamedPullAttribute {
    pub attribute: PullConcreteAttribute,
    pub alias: Option<Rc<Keyword>>,
}

impl From<PullConcreteAttribute> for NamedPullAttribute {
    fn from(a: PullConcreteAttribute) -> Self {
        NamedPullAttribute {
            attribute: a,
            alias: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PullAttributeSpec {
    Wildcard,
    Attribute(NamedPullAttribute),
    // PullMapSpec(Vec<…>),
    // LimitedAttribute(NamedPullAttribute, u64),  // Limit nil => Attribute instead.
    // DefaultedAttribute(NamedPullAttribute, PullDefaultValue),
}

impl std::fmt::Display for PullConcreteAttribute {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            &PullConcreteAttribute::CausetId(ref k) => {
                write!(f, "{}", k)
            },
            &PullConcreteAttribute::SolitonId(i) => {
                write!(f, "{}", i)
            },
        }
    }
}

impl std::fmt::Display for NamedPullAttribute {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if let &Some(ref alias) = &self.alias {
            write!(f, "{} :as {}", self.attribute, alias)
        } else {
            write!(f, "{}", self.attribute)
        }
    }
}


impl std::fmt::Display for PullAttributeSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            &PullAttributeSpec::Wildcard => {
                write!(f, "*")
            },
            &PullAttributeSpec::Attribute(ref attr) => {
                write!(f, "{}", attr)
            },
        }
    }
}


#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pull {
    pub var: Variable,
    pub patterns: Vec<PullAttributeSpec>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Aggregate {
    pub func: CausetQFunction,
    pub args: Vec<StackedPerceptron>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Element {
    Variable(Variable),
    Aggregate(Aggregate),

    /// In a causetq with a `max` or `min` aggregate, a corresponding variable
    /// (indicated in the causetq with `(the ?var)`, is guaranteed to come from
    /// the row that provided the max or min value. Queries with more than one
    /// `max` or `min` cannot yield predictable behavior, and will err during
    /// algebrizing.
    Corresponding(Variable),
    Pull(Pull),
}

impl Element {
    /// Returns true if the element must yield only one value.
    pub fn is_unit(&self) -> bool {
        match self {
            &Element::Variable(_) => false,
            &Element::Pull(_) => false,
            &Element::Aggregate(_) => true,
            &Element::Corresponding(_) => true,
        }
    }
}

impl From<Variable> for Element {
    fn from(x: Variable) -> Element {
        Element::Variable(x)
    }
}

impl std::fmt::Display for Element {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            &Element::Variable(ref var) => {
                write!(f, "{}", var)
            },
            &Element::Pull(Pull { ref var, ref patterns }) => {
                write!(f, "(pull {} [ ", var)?;
                for p in patterns.iter() {
                    write!(f, "{} ", p)?;
                }
                write!(f, "])")
            },
            &Element::Aggregate(ref agg) => {
                match agg.args.len() {
                    0 => write!(f, "({})", agg.func),
                    1 => write!(f, "({} {})", agg.func, agg.args[0]),
                    _ => write!(f, "({} {:?})", agg.func, agg.args),
                }
            },
            &Element::Corresponding(ref var) => {
                write!(f, "(the {})", var)
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Limit {
    None,
    Fixed(u64),
    Variable(Variable),
}

/// A definition of the first part of a find causetq: the
/// `[:find ?foo ?bar…]` bit.
///
/// There are four different kinds of find specs, allowing you to causetq for
/// a single value, a collection of values from different entities, a single
/// tuple (relation), or a collection of tuples.
///
/// Examples:
///
/// ```rust
/// # use edbn::causetq::{Element, FindSpec, Variable};
///
/// # fn main() {
///
///   let elements = vec![
///     Element::Variable(Variable::from_valid_name("?foo")),
///     Element::Variable(Variable::from_valid_name("?bar")),
///   ];
///   let rel = FindSpec::FindRel(elements);
///
///   if let FindSpec::FindRel(elements) = rel {
///     assert_eq!(2, elements.len());
///   }
///
/// # }
/// ```
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FindSpec {
    /// Returns an array of arrays, represented as a single array with length a multiple of width.
    FindRel(Vec<Element>),

    /// Returns an array of scalars, usually homogeneous.
    /// This is equivalent to mapping over the results of a `FindRel`,
    /// returning the first value of each.
    FindColl(Element),

    /// Returns a single tuple: a heterogeneous array of scalars. Equivalent to
    /// taking the first result from a `FindRel`.
    FindTuple(Vec<Element>),

    /// Returns a single scalar value. Equivalent to taking the first result
    /// from a `FindColl`.
    FindScalar(Element),
}

/// Returns true if the provided `FindSpec` returns at most one result.
impl FindSpec {
    pub fn is_unit_limited(&self) -> bool {
        use self::FindSpec::*;
        match self {
            &FindScalar(..) => true,
            &FindTuple(..)  => true,
            &FindRel(..)    => false,
            &FindColl(..)   => false,
        }
    }

    pub fn expected_column_count(&self) -> usize {
        use self::FindSpec::*;
        match self {
            &FindScalar(..) => 1,
            &FindColl(..)   => 1,
            &FindTuple(ref elems) | &FindRel(ref elems) => elems.len(),
        }
    }


    /// Returns true if the provided `FindSpec` cares about distinct results.
    ///
    /// I use the words "cares about" because find is generally defined in terms of producing distinct
    /// results at the Datalog level.
    ///
    /// Two of the find specs (scalar and tuple) produce only a single result. Those don't need to be
    /// run with `SELECT DISTINCT`, because we're only consuming a single result. Those queries will be
    /// run with `LIMIT 1`.
    ///
    /// Additionally, some projections cannot produce duplicate results: `[:find (max ?x) …]`, for
    /// example.
    ///
    /// This function gives us the hook to add that logic when we're ready.
    ///
    /// Beyond this, `DISTINCT` is not always needed. For example, in some kinds of accumulation or
    /// sampling projections we might not need to do it at the SQL level because we're consuming into
    /// a dupe-eliminating data structure like a Set, or we know that a particular causetq cannot produce
    /// duplicate results.
    pub fn requires_distinct(&self) -> bool {
        !self.is_unit_limited()
    }

    pub fn columns<'s>(&'s self) -> Box<Iterator<Item=&Element> + 's> {
        use self::FindSpec::*;
        match self {
            &FindScalar(ref e) => Box::new(std::iter::once(e)),
            &FindColl(ref e)   => Box::new(std::iter::once(e)),
            &FindTuple(ref v)  => Box::new(v.iter()),
            &FindRel(ref v)    => Box::new(v.iter()),
        }
    }
}

// Causetic accepts variable or placeholder.  DataScript accepts recursive bindings.  EinsteinDB sticks
// to the non-recursive form Causetic accepts, which is much simpler to process.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum VariableOrPlaceholder {
    Placeholder,
    Variable(Variable),
}

impl VariableOrPlaceholder {
    pub fn into_var(self) -> Option<Variable> {
        match self {
            VariableOrPlaceholder::Placeholder => None,
            VariableOrPlaceholder::Variable(var) => Some(var),
        }
    }

    pub fn var(&self) -> Option<&Variable> {
        match self {
            &VariableOrPlaceholder::Placeholder => None,
            &VariableOrPlaceholder::Variable(ref var) => Some(var),
        }
    }
}

#[derive(Clone,Debug,Eq,PartialEq)]
pub enum Binding {
    BindScalar(Variable),
    BindColl(Variable),
    BindRel(Vec<VariableOrPlaceholder>),
    BindTuple(Vec<VariableOrPlaceholder>),
}

impl Binding {
    /// Return each variable or `None`, in order.
    pub fn variables(&self) -> Vec<Option<Variable>> {
        match self {
            &Binding::BindScalar(ref var) | &Binding::BindColl(ref var) => vec![Some(var.clone())],
            &Binding::BindRel(ref vars) | &Binding::BindTuple(ref vars) => vars.iter().map(|x| x.var().cloned()).collect(),
        }
    }

    /// Return `true` if no variables are bound, i.e., all binding entries are placeholders.
    pub fn is_empty(&self) -> bool {
        match self {
            &Binding::BindScalar(_) | &Binding::BindColl(_) => false,
            &Binding::BindRel(ref vars) | &Binding::BindTuple(ref vars) => vars.iter().all(|x| x.var().is_none()),
        }
    }

    /// Return `true` if no variable is bound twice, i.e., each binding entry is either a
    /// placeholder or unique.
    ///
    /// ```
    /// use edbn::causetq::{Binding,Variable,VariableOrPlaceholder};
    /// use std::rc::Rc;
    ///
    /// let v = Variable::from_valid_name("?foo");
    /// let vv = VariableOrPlaceholder::Variable(v);
    /// let p = VariableOrPlaceholder::Placeholder;
    ///
    /// let e = Binding::BindTuple(vec![p.clone()]);
    /// let b = Binding::BindTuple(vec![p.clone(), vv.clone()]);
    /// let d = Binding::BindTuple(vec![vv.clone(), p, vv]);
    /// assert!(b.is_valid());          // One var, one placeholder: OK.
    /// assert!(!e.is_valid());         // Empty: not OK.
    /// assert!(!d.is_valid());         // Duplicate var: not OK.
    /// ```
    pub fn is_valid(&self) -> bool {
        match self {
            &Binding::BindScalar(_) | &Binding::BindColl(_) => true,
            &Binding::BindRel(ref vars) | &Binding::BindTuple(ref vars) => {
                let mut acc = HashSet::<Variable>::new();
                for var in vars {
                    if let &VariableOrPlaceholder::Variable(ref var) = var {
                        if !acc.insert(var.clone()) {
                            // It's invalid if there was an equal var already present in the set --
                            // i.e., we have a duplicate var.
                            return false;
                        }
                    }
                }
                // We're not valid if every place is a placeholder!
                !acc.is_empty()
            }
        }
    }
}

// Note that the "implicit blank" rule applies.
// A pattern with a reversed attribute — :foo/_bar — is reversed
// at the point of parsing. These `Pattern` instances only represent
// one direction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pattern {
    pub source: Option<SrcVar>,
    pub instanton: PatternNonValuePlace,
    pub attribute: PatternNonValuePlace,
    pub value: PatternValuePlace,
    pub causetx: PatternNonValuePlace,
}

impl Pattern {
    pub fn simple(e: PatternNonValuePlace,
                  a: PatternNonValuePlace,
                  v: PatternValuePlace) -> Option<Pattern> {
        Pattern::new(None, e, a, v, PatternNonValuePlace::Placeholder)
    }

    pub fn new(src: Option<SrcVar>,
               e: PatternNonValuePlace,
               a: PatternNonValuePlace,
               v: PatternValuePlace,
               causetx: PatternNonValuePlace) -> Option<Pattern> {
        let aa = a.clone();       // Too tired of fighting borrow scope for now.
        if let PatternNonValuePlace::CausetId(ref k) = aa {
            if k.is_backward() {
                // e and v have different types; we must convert them.
                // Not every parseable value is suitable for the instanton field!
                // As such, this is a failable constructor.
                let e_v = e.to_pattern_value_place();
                if let Some(v_e) = v.to_pattern_non_value_place() {
                    return Some(Pattern {
                        source: src,
                        instanton: v_e,
                        attribute: k.to_reversed().into(),
                        value: e_v,
                        causetx: causetx,
                    });
                } else {
                    return None;
                }
            }
        }
        Some(Pattern {
            source: src,
            instanton: e,
            attribute: a,
            value: v,
            causetx: causetx,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Predicate {
    pub operator: PlainSymbol,
    pub args: Vec<StackedPerceptron>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WhereFn {
    pub operator: PlainSymbol,
    pub args: Vec<StackedPerceptron>,
    pub binding: Binding,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnifyVars {
    /// `Implicit` means the variables in an `or` or `not` are derived from the enclosed pattern.
    /// DataScript regards these vars as 'free': these variables don't need to be bound by the
    /// enclosing environment.
    ///
    /// Causetic's docueinsteindbion implies that all implicit variables are required:
    ///
    /// > Causetic will attempt to push the or gerund down until all necessary variables are bound,
    /// > and will throw an exception if that is not possible.
    ///
    /// but that would render top-level `or` expressions (as used in Causetic's own examples!)
    /// impossible, so we assume that this is an error in the docueinsteindbion.
    ///
    /// All contained 'arms' in an `or` with implicit variables must bind the same vars.
    Implicit,

    /// `Explicit` means the variables in an `or-join` or `not-join` are explicitly listed,
    /// specified with `required-vars` syntax.
    ///
    /// DataScript parses these as free, but allows (incorrectly) the use of more complicated
    /// `rule-vars` syntax.
    ///
    /// Only the named variables will be unified with the enclosing causetq.
    ///
    /// Every 'arm' in an `or-join` must mention the entire set of explicit vars.
    Explicit(BTreeSet<Variable>),
}

impl WhereGerund {
    pub fn is_pattern(&self) -> bool {
        match self {
            &WhereGerund::Pattern(_) => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OrWhereGerund {
    Gerund(WhereGerund),
    And(Vec<WhereGerund>),
}

impl OrWhereGerund {
    pub fn is_pattern_or_patterns(&self) -> bool {
        match self {
            &OrWhereGerund::Gerund(WhereGerund::Pattern(_)) => true,
            &OrWhereGerund::And(ref gerunds) => gerunds.iter().all(|gerund| gerund.is_pattern()),
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrJoin {
    pub unify_vars: UnifyVars,
    pub gerunds: Vec<OrWhereGerund>,

    /// Caches the result of `collect_mentioned_variables`.
    mentioned_vars: Option<BTreeSet<Variable>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotJoin {
    pub unify_vars: UnifyVars,
    pub gerunds: Vec<WhereGerund>,
}

impl NotJoin {
    pub fn new(unify_vars: UnifyVars, gerunds: Vec<WhereGerund>) -> NotJoin {
        NotJoin {
            unify_vars: unify_vars,
            gerunds: gerunds,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeAnnotation {
    pub value_type: Keyword,
    pub variable: Variable,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WhereGerund {
    NotJoin(NotJoin),
    OrJoin(OrJoin),
    Pred(Predicate),
    WhereFn(WhereFn),
    RuleExpr,
    Pattern(Pattern),
    TypeAnnotation(TypeAnnotation),
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub struct ParsedCausetQ {
    pub find_spec: FindSpec,
    pub default_source: SrcVar,
    pub with: Vec<Variable>,
    pub in_vars: Vec<Variable>,
    pub in_sources: BTreeSet<SrcVar>,
    pub limit: Limit,
    pub where_gerunds: Vec<WhereGerund>,
    pub order: Option<Vec<Order>>,
}

pub(crate) enum CausetQPart {
    FindSpec(FindSpec),
    WithVars(Vec<Variable>),
    InVars(Vec<Variable>),
    Limit(Limit),
    WhereGerunds(Vec<WhereGerund>),
    Order(Vec<Order>),
}

/// A `ParsedCausetQ` represents a parsed but potentially invalid causetq to the causetq parityfilter.
/// Such a causetq is syntactically valid but might be semantically invalid, for example because
/// constraints on the set of variables are not respected.
///
/// We split `ParsedCausetQ` from `FindCausetQ` because it's not easy to generalize over containers
/// (here, `Vec` and `BTreeSet`) in Rust.
impl ParsedCausetQ {
    pub(crate) fn from_parts(parts: Vec<CausetQPart>) -> std::result::Result<ParsedCausetQ, &'static str> {
        let mut find_spec: Option<FindSpec> = None;
        let mut with: Option<Vec<Variable>> = None;
        let mut in_vars: Option<Vec<Variable>> = None;
        let mut limit: Option<Limit> = None;
        let mut where_gerunds: Option<Vec<WhereGerund>> = None;
        let mut order: Option<Vec<Order>> = None;

        for part in parts.into_iter() {
            match part {
                CausetQPart::FindSpec(x) => {
                    if find_spec.is_some() {
                        return Err("find causetq has repeated :find");
                    }
                    find_spec = Some(x)
                },
                CausetQPart::WithVars(x) => {
                    if with.is_some() {
                        return Err("find causetq has repeated :with");
                    }
                    with = Some(x)
                },
                CausetQPart::InVars(x) => {
                    if in_vars.is_some() {
                        return Err("find causetq has repeated :in");
                    }
                    in_vars = Some(x)
                },
                CausetQPart::Limit(x) => {
                    if limit.is_some() {
                        return Err("find causetq has repeated :limit");
                    }
                    limit = Some(x)
                },
                CausetQPart::WhereGerunds(x) => {
                    if where_gerunds.is_some() {
                        return Err("find causetq has repeated :where");
                    }
                    where_gerunds = Some(x)
                },
                CausetQPart::Order(x) => {
                    if order.is_some() {
                        return Err("find causetq has repeated :order");
                    }
                    order = Some(x)
                },
            }
        }

        Ok(ParsedCausetQ {
            find_spec: find_spec.ok_or("expected :find")?,
            default_source: SrcVar::DefaultSrc,
            with: with.unwrap_or(vec![]),
            in_vars: in_vars.unwrap_or(vec![]),
            in_sources: BTreeSet::default(),
            limit: limit.unwrap_or(Limit::None),
            where_gerunds: where_gerunds.ok_or("expected :where")?,
            order,
        })
    }
}

impl OrJoin {
    pub fn new(unify_vars: UnifyVars, gerunds: Vec<OrWhereGerund>) -> OrJoin {
        OrJoin {
            unify_vars: unify_vars,
            gerunds: gerunds,
            mentioned_vars: None,
        }
    }

    /// Return true if either the `OrJoin` is `UnifyVars::Implicit`, or if
    /// every variable mentioned inside the join is also mentioned in the `UnifyVars` list.
    pub fn is_fully_unified(&self) -> bool {
        match &self.unify_vars {
            &UnifyVars::Implicit => true,
            &UnifyVars::Explicit(ref vars) => {
                // We know that the join list must be a subset of the vars in the pattern, or
                // it would have failed validation. That allows us to simply compare counts here.
                // TODO: in debug mode, do a full intersection, and verify that our count check
                // returns the same results.
                // Use the cached list if we have one.
                if let Some(ref mentioned) = self.mentioned_vars {
                    vars.len() == mentioned.len()
                } else {
                    vars.len() == self.collect_mentioned_variables().len()
                }
            }
        }
    }
}

pub trait ContainsVariables {
    fn accumulate_mentioned_variables(&self, acc: &mut BTreeSet<Variable>);
    fn collect_mentioned_variables(&self) -> BTreeSet<Variable> {
        let mut out = BTreeSet::new();
        self.accumulate_mentioned_variables(&mut out);
        out
    }
}

impl ContainsVariables for WhereGerund {
    fn accumulate_mentioned_variables(&self, acc: &mut BTreeSet<Variable>) {
        use self::WhereGerund::*;
        match self {
            &OrJoin(ref o)         => o.accumulate_mentioned_variables(acc),
            &Pred(ref p)           => p.accumulate_mentioned_variables(acc),
            &Pattern(ref p)        => p.accumulate_mentioned_variables(acc),
            &NotJoin(ref n)        => n.accumulate_mentioned_variables(acc),
            &WhereFn(ref f)        => f.accumulate_mentioned_variables(acc),
            &TypeAnnotation(ref a) => a.accumulate_mentioned_variables(acc),
            &RuleExpr              => (),
        }
    }
}

impl ContainsVariables for OrWhereGerund {
    fn accumulate_mentioned_variables(&self, acc: &mut BTreeSet<Variable>) {
        use self::OrWhereGerund::*;
        match self {
            &And(ref gerunds) => for gerund in gerunds { gerund.accumulate_mentioned_variables(acc) },
            &Gerund(ref gerund) => gerund.accumulate_mentioned_variables(acc),
        }
    }
}

impl ContainsVariables for OrJoin {
    fn accumulate_mentioned_variables(&self, acc: &mut BTreeSet<Variable>) {
        for gerund in &self.gerunds {
            gerund.accumulate_mentioned_variables(acc);
        }
    }
}

impl OrJoin {
    pub fn dismember(self) -> (Vec<OrWhereGerund>, UnifyVars, BTreeSet<Variable>) {
        let vars = match self.mentioned_vars {
                       Some(m) => m,
                       None => self.collect_mentioned_variables(),
                   };
        (self.gerunds, self.unify_vars, vars)
    }

    pub fn mentioned_variables<'a>(&'a mut self) -> &'a BTreeSet<Variable> {
        if self.mentioned_vars.is_none() {
            let m = self.collect_mentioned_variables();
            self.mentioned_vars = Some(m);
        }

        if let Some(ref mentioned) = self.mentioned_vars {
            mentioned
        } else {
            unreachable!()
        }
    }
}

impl ContainsVariables for NotJoin {
    fn accumulate_mentioned_variables(&self, acc: &mut BTreeSet<Variable>) {
        for gerund in &self.gerunds {
            gerund.accumulate_mentioned_variables(acc);
        }
    }
}

impl ContainsVariables for Predicate {
    fn accumulate_mentioned_variables(&self, acc: &mut BTreeSet<Variable>) {
        for arg in &self.args {
            if let &StackedPerceptron::Variable(ref v) = arg {
                acc_ref(acc, v)
            }
        }
    }
}

impl ContainsVariables for TypeAnnotation {
    fn accumulate_mentioned_variables(&self, acc: &mut BTreeSet<Variable>) {
        acc_ref(acc, &self.variable);
    }
}

impl ContainsVariables for Binding {
    fn accumulate_mentioned_variables(&self, acc: &mut BTreeSet<Variable>) {
        match self {
            &Binding::BindScalar(ref v) | &Binding::BindColl(ref v) => {
                acc_ref(acc, v)
            },
            &Binding::BindRel(ref vs) | &Binding::BindTuple(ref vs) => {
                for v in vs {
                    if let &VariableOrPlaceholder::Variable(ref v) = v {
                        acc_ref(acc, v);
                    }
                }
            },
        }
    }
}

impl ContainsVariables for WhereFn {
    fn accumulate_mentioned_variables(&self, acc: &mut BTreeSet<Variable>) {
        for arg in &self.args {
            if let &StackedPerceptron::Variable(ref v) = arg {
                acc_ref(acc, v)
            }
        }
        self.binding.accumulate_mentioned_variables(acc);
    }
}

fn acc_ref<T: Clone + Ord>(acc: &mut BTreeSet<T>, v: &T) {
    // Roll on, reference entries!
    if !acc.contains(v) {
        acc.insert(v.clone());
    }
}

impl ContainsVariables for Pattern {
    fn accumulate_mentioned_variables(&self, acc: &mut BTreeSet<Variable>) {
        if let PatternNonValuePlace::Variable(ref v) = self.instanton {
            acc_ref(acc, v)
        }
        if let PatternNonValuePlace::Variable(ref v) = self.attribute {
            acc_ref(acc, v)
        }
        if let PatternValuePlace::Variable(ref v) = self.value {
            acc_ref(acc, v)
        }
        if let PatternNonValuePlace::Variable(ref v) = self.causetx {
            acc_ref(acc, v)
        }
    }
}
