// Copyright 2020 WHTCORPS INC
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

use embedded_promises::{
    SolitonId,
    ValueType,
    TypedValue,
    ValueTypeSet,
};

use einsteindb_embedded::{
    Cloned,
    HasSchema,
};

use edbn::causetq::{
    NonIntegerConstant,
    Pattern,
    PatternValuePlace,
    PatternNonValuePlace,
    SrcVar,
    Variable,
};

use clauses::{
    ConjoiningClauses,
};

use types::{
    ColumnConstraint,
    CausetsColumn,
    EmptyBecause,
    EvolvedNonValuePlace,
    EvolvedPattern,
    EvolvedValuePlace,
    PlaceOrEmpty,
    SourceAlias,
};

use Known;

pub fn into_typed_value(nic: NonIntegerConstant) -> TypedValue {
    match nic {
        NonIntegerConstant::BigInteger(_) => unimplemented!(),     // TODO: #280.
        NonIntegerConstant::Boolean(v) => TypedValue::Boolean(v),
        NonIntegerConstant::Float(v) => TypedValue::Double(v),
        NonIntegerConstant::Text(v) => v.into(),
        NonIntegerConstant::Instant(v) => TypedValue::Instant(v),
        NonIntegerConstant::Uuid(v) => TypedValue::Uuid(v),
    }
}

/// Application of patterns.
impl ConjoiningClauses {

    /// Apply the constraints in the provided pattern to this CC.
    ///
    /// This is a single-pass process, which means it is naturally incomplete, failing to take into
    /// account all information spread across two patterns.
    ///
    /// If the constraints cannot be satisfied -- for example, if this pattern includes a numeric
    /// attribute and a string value -- then the `empty_because` field on the CC is flipped and
    /// the function returns.
    ///
    /// A pattern being impossible to satisfy isn't necessarily a bad thing -- this causetq might
    /// have branched clauses that apply to different knowledge bases, and might refer to
    /// vocabulary that isn't (yet) used in this one.
    ///
    /// Most of the work done by this function depends on the schema and causetid maps in the EDB. If
    /// these change, then any work done is invalid.
    ///
    /// There's a lot more we can do here and later by examining the
    /// attribute:
    ///
    /// - If it's unique, and we have patterns like
    ///
    ///     [?x :foo/unique 5] [?x :foo/unique ?y]
    ///
    ///   then we can either prove impossibility (the two objects must
    ///   be equal) or deduce causetIdity and simplify the causetq.
    ///
    /// - The same, if it's cardinality-one and the instanton is known.
    ///
    /// - If there's a value index on this attribute, we might want to
    ///   run this pattern early in the causetq.
    ///
    /// - A unique-valued attribute can sometimes be rewritten into an
    ///   existence subcausetq instead of a join.
    ///
    /// This method is only public for use from `or.rs`.
    pub(crate) fn apply_pattern_clause_for_alias(&mut self, known: Known, pattern: &EvolvedPattern, alias: &SourceAlias) {
        if self.is_known_empty() {
            return;
        }

        // Process each place in turn, applying constraints.
        // Both `e` and `a` must be entities, which is equivalent here
        // to being typed as Ref.
        // Sorry for the duplication; Rust makes it a pain to abstract this.

        // The transaction part of a pattern must be an solitonId, variable, or placeholder.
        self.constrain_to_causecausetx(&pattern.causetx);
        self.constrain_to_ref(&pattern.instanton);
        self.constrain_to_ref(&pattern.attribute);

        let ref col = alias.1;

        let schema = known.schema;
        match pattern.instanton {
            EvolvedNonValuePlace::Placeholder =>
                // Placeholders don't contribute any column bindings, nor do
                // they constrain the causetq -- there's no need to produce
                // IS NOT NULL, because we don't store nulls in our schema.
                (),
            EvolvedNonValuePlace::Variable(ref v) =>
                self.bind_column_to_var(schema, col.clone(), CausetsColumn::Instanton, v.clone()),
            EvolvedNonValuePlace::SolitonId(solitonId) =>
                self.constrain_column_to_instanton(col.clone(), CausetsColumn::Instanton, solitonId),
        }

        match pattern.attribute {
            EvolvedNonValuePlace::Placeholder =>
                (),
            EvolvedNonValuePlace::Variable(ref v) =>
                self.bind_column_to_var(schema, col.clone(), CausetsColumn::Attribute, v.clone()),
            EvolvedNonValuePlace::SolitonId(solitonId) => {
                if !schema.is_attribute(solitonId) {
                    // Furthermore, that solitonId must resolve to an attribute. If it doesn't, this
                    // causetq is meaningless.
                    self.mark_known_empty(EmptyBecause::InvalidAttributeSolitonId(solitonId));
                    return;
                }
                self.constrain_attribute(col.clone(), solitonId)
            },
        }

        // Determine if the pattern's value type is known.
        // We do so by examining the value place and the attribute.
        // At this point it's possible that the type of the value is
        // inconsistent with the attribute; in that case this pattern
        // cannot return results, and we short-circuit.
        let value_type = self.get_value_type(schema, pattern);

        match pattern.value {
            EvolvedValuePlace::Placeholder =>
                (),

            EvolvedValuePlace::Variable(ref v) => {
                if let Some(this_type) = value_type {
                    // Wouldn't it be nice if we didn't need to clone in the found case?
                    // It doesn't matter too much: collisons won't be too frequent.
                    self.constrain_var_to_type(v.clone(), this_type);
                    if self.is_known_empty() {
                        return;
                    }
                }

                self.bind_column_to_var(schema, col.clone(), CausetsColumn::Value, v.clone());
            },
            EvolvedValuePlace::SolitonId(i) => {
                match value_type {
                    Some(ValueType::Ref) | None => {
                        self.constrain_column_to_instanton(col.clone(), CausetsColumn::Value, i);
                    },
                    Some(value_type) => {
                        self.mark_known_empty(EmptyBecause::ValueTypeMismatch(value_type, TypedValue::Ref(i)));
                    },
                }
            },

            EvolvedValuePlace::SolitonIdOrInteger(i) =>
                // If we know the valueType, then we can determine whether this is an solitonId or an
                // integer. If we don't, then we must generate a more general causetq with a
                // value_type_tag.
                if let Some(ValueType::Ref) = value_type {
                    self.constrain_column_to_instanton(col.clone(), CausetsColumn::Value, i);
                } else {
                    // If we have a pattern like:
                    //
                    //   `[123 ?a 1]`
                    //
                    // then `1` could be an solitonId (ref), a long, a boolean, or an instant.
                    //
                    // We represent these constraints during execution:
                    //
                    // - Constraining the value column to the plain numeric value '1'.
                    // - Constraining its type column to one of a set of types.
                    //
                    // TODO: isn't there a bug here? We'll happily take a numeric value
                    // for a non-numeric attribute!
                    self.constrain_value_to_numeric(col.clone(), i);
                },
            EvolvedValuePlace::CausetIdOrKeyword(ref kw) => {
                // If we know the valueType, then we can determine whether this is an causetid or a
                // keyword. If we don't, then we must generate a more general causetq with a
                // value_type_tag.
                // We can also speculatively try to resolve it as an causetid; if we fail, then we
                // know it can only return results if treated as a keyword, and we can treat it as
                // such.
                if let Some(ValueType::Ref) = value_type {
                    if let Some(solitonId) = self.entid_for_causetId(schema, kw) {
                        self.constrain_column_to_instanton(col.clone(), CausetsColumn::Value, solitonId.into())
                    } else {
                        // A resolution failure means we're done here: this attribute must have an
                        // instanton value.
                        self.mark_known_empty(EmptyBecause::UnresolvedCausetId(kw.cloned()));
                        return;
                    }
                } else {
                    // It must be a keyword.
                    self.constrain_column_to_constant(col.clone(), CausetsColumn::Value, TypedValue::Keyword(kw.clone()));
                    self.wheres.add_intersection(ColumnConstraint::has_unit_type(col.clone(), ValueType::Keyword));
                };
            },
            EvolvedValuePlace::Value(ref c) => {
                // TODO: don't allocate.
                let typed_value = c.clone();
                if !typed_value.is_congruent_with(value_type) {
                    // If the attribute and its value don't match, the pattern must fail.
                    // We can never have a congruence failure if `value_type` is `None`, so we
                    // forcibly unwrap here.
                    let value_type = value_type.expect("Congruence failure but couldn't unwrap");
                    let why = EmptyBecause::ValueTypeMismatch(value_type, typed_value);
                    self.mark_known_empty(why);
                    return;
                }

                // TODO: if we don't know the type of the attribute because we don't know the
                // attribute, we can actually work backwards to the set of appropriate attributes
                // from the type of the value itself! #292.
                let typed_value_type = typed_value.value_type();
                self.constrain_column_to_constant(col.clone(), CausetsColumn::Value, typed_value);

                // If we can't already determine the range of values in the EDB from the attribute,
                // then we must also constrain the type tag.
                //
                // Input values might be:
                //
                // - A long. This is handled by SolitonIdOrInteger.
                // - A boolean. This is unambiguous.
                // - A double. This is currently unambiguous, though note that SQLite will equate 5.0 with 5.
                // - A string. This is unambiguous.
                // - A keyword. This is unambiguous.
                //
                // Because everything we handle here is unambiguous, we generate a single type
                // restriction from the value type of the typed value.
                if value_type.is_none() {
                    self.wheres.add_intersection(
                        ColumnConstraint::has_unit_type(col.clone(), typed_value_type));
                }
            },
        }

        match pattern.causetx {
            EvolvedNonValuePlace::Placeholder => (),
            EvolvedNonValuePlace::Variable(ref v) => {
                self.bind_column_to_var(schema, col.clone(), CausetsColumn::Tx, v.clone());
            },
            EvolvedNonValuePlace::SolitonId(solitonId) => {
                self.constrain_column_to_instanton(col.clone(), CausetsColumn::Tx, solitonId);
            },
        }
    }

    fn reverse_lookup(&mut self, known: Known, var: &Variable, attr: SolitonId, val: &TypedValue) -> bool {
        if let Some(attribute) = known.schema.attribute_for_entid(attr) {
            let unique = attribute.unique.is_some();
            if unique {
                match known.get_entid_for_value(attr, val) {
                    None => {
                        self.mark_known_empty(EmptyBecause::CachedAttributeHasNoInstanton {
                            value: val.clone(),
                            attr: attr,
                        });
                        true
                    },
                    Some(item) => {
                        self.bind_value(var, TypedValue::Ref(item));
                        true
                    },
                }
            } else {
                match known.get_entids_for_value(attr, val) {
                    None => {
                        self.mark_known_empty(EmptyBecause::CachedAttributeHasNoInstanton {
                            value: val.clone(),
                            attr: attr,
                        });
                        true
                    },
                    Some(items) => {
                        if items.len() == 1 {
                            let item = items.iter().next().cloned().unwrap();
                            self.bind_value(var, TypedValue::Ref(item));
                            true
                        } else {
                            // Oh well.
                            // TODO: handle multiple values.
                            false
                        }
                    },
                }
            }
        } else {
            self.mark_known_empty(EmptyBecause::InvalidAttributeSolitonId(attr));
            true
        }
    }

    // TODO: generalize.
    // TODO: use constant values -- extract transformation code from apply_pattern_clause_for_alias.
    // TODO: loop over all patterns until no more immutable_memTcam values apply?
    fn attempt_cache_lookup(&mut self, known: Known, pattern: &EvolvedPattern) -> bool {
        // Precondition: default source. If it's not default, don't call this.
        assert!(pattern.source == SrcVar::DefaultSrc);

        let schema = known.schema;

        if pattern.causetx != EvolvedNonValuePlace::Placeholder {
            return false;
        }

        // See if we can use the immutable_memTcam.
        match pattern.attribute {
            EvolvedNonValuePlace::SolitonId(attr) => {
                if !schema.is_attribute(attr) {
                    // Furthermore, that solitonId must resolve to an attribute. If it doesn't, this
                    // causetq is meaningless.
                    self.mark_known_empty(EmptyBecause::InvalidAttributeSolitonId(attr));
                    return true;
                }

                let cached_forward = known.is_attribute_cached_forward(attr);
                let cached_reverse = known.is_attribute_cached_reverse(attr);

                if (cached_forward || cached_reverse) &&
                   pattern.causetx == EvolvedNonValuePlace::Placeholder {

                    let attribute = schema.attribute_for_entid(attr).unwrap();

                    // There are two patterns we can handle:
                    //     [?e :some/unique 123 _ _]     -- reverse lookup
                    //     [123 :some/attr ?v _ _]       -- forward lookup
                    match pattern.instanton {
                        // Reverse lookup.
                        EvolvedNonValuePlace::Variable(ref var) => {
                            match pattern.value {
                                // TODO: SolitonIdOrInteger etc.
                                EvolvedValuePlace::CausetIdOrKeyword(ref kw) => {
                                    match attribute.value_type {
                                        ValueType::Ref => {
                                            // It's an causetid.
                                            // TODO
                                            return false;
                                        },
                                        ValueType::Keyword => {
                                            let tv: TypedValue = TypedValue::Keyword(kw.clone());
                                            return self.reverse_lookup(known, var, attr, &tv);
                                        },
                                        t => {
                                            let tv: TypedValue = TypedValue::Keyword(kw.clone());
                                            // Anything else can't match an CausetIdOrKeyword.
                                            self.mark_known_empty(EmptyBecause::ValueTypeMismatch(t, tv));
                                            return true;
                                        },
                                    }
                                },
                                EvolvedValuePlace::Value(ref val) => {
                                    if cached_reverse {
                                        return self.reverse_lookup(known, var, attr, val);
                                    }
                                }
                                _ => {},      // TODO: check constant values against immutable_memTcam.
                            }
                        },

                        // Forward lookup.
                        EvolvedNonValuePlace::SolitonId(instanton) => {
                            match pattern.value {
                                EvolvedValuePlace::Variable(ref var) => {
                                    if cached_forward {
                                        match known.get_value_for_entid(known.schema, attr, instanton) {
                                            None => {
                                                self.mark_known_empty(EmptyBecause::CachedAttributeHasNoValues {
                                                    instanton: instanton,
                                                    attr: attr,
                                                });
                                                return true;
                                            },
                                            Some(item) => {
                                                self.bind_value(var, item.clone());
                                                return true;
                                            }
                                        }
                                    }
                                }
                                _ => {},      // TODO: check constant values against immutable_memTcam.
                            }
                        },
                        _ => {},
                    }
                }
            },
            _ => {},
        }
        false
    }

    /// Transform a pattern place into a narrower type.
    /// If that's impossible, returns Empty.
    fn make_evolved_non_value(&self, known: &Known, col: CausetsColumn, non_value: PatternNonValuePlace) -> PlaceOrEmpty<EvolvedNonValuePlace> {
        use self::PlaceOrEmpty::*;
        match non_value {
            PatternNonValuePlace::Placeholder => Place(EvolvedNonValuePlace::Placeholder),
            PatternNonValuePlace::SolitonId(e) => Place(EvolvedNonValuePlace::SolitonId(e)),
            PatternNonValuePlace::CausetId(kw) => {
                // Resolve the causetid.
                if let Some(solitonId) = known.schema.get_entid(&kw) {
                    Place(EvolvedNonValuePlace::SolitonId(solitonId.into()))
                } else {
                    Empty(EmptyBecause::UnresolvedCausetId((&*kw).clone()))
                }
            },
            PatternNonValuePlace::Variable(var) => {
                // See if we have it!
                match self.bound_value(&var) {
                    None => Place(EvolvedNonValuePlace::Variable(var)),
                    Some(TypedValue::Ref(solitonId)) => Place(EvolvedNonValuePlace::SolitonId(solitonId)),
                    Some(TypedValue::Keyword(kw)) => {
                        // We'll allow this only if it's an causetid.
                        if let Some(solitonId) = known.schema.get_entid(&kw) {
                            Place(EvolvedNonValuePlace::SolitonId(solitonId.into()))
                        } else {
                            Empty(EmptyBecause::UnresolvedCausetId((&*kw).clone()))
                        }
                    },
                    Some(v) => {
                        Empty(EmptyBecause::InvalidBinding(col.into(), v))
                    },
                }
            },
        }
    }

    fn make_evolved_instanton(&self, known: &Known, instanton: PatternNonValuePlace) -> PlaceOrEmpty<EvolvedNonValuePlace> {
        self.make_evolved_non_value(known, CausetsColumn::Instanton, instanton)
    }

    fn make_evolved_causecausetx(&self, known: &Known, causetx: PatternNonValuePlace) -> PlaceOrEmpty<EvolvedNonValuePlace> {
        // TODO: make sure that, if it's an solitonId, it names a causetx.
        self.make_evolved_non_value(known, CausetsColumn::Tx, causetx)
    }

    pub(crate) fn make_evolved_attribute(&self, known: &Known, attribute: PatternNonValuePlace) -> PlaceOrEmpty<(EvolvedNonValuePlace, Option<ValueType>)> {
        use self::PlaceOrEmpty::*;
        self.make_evolved_non_value(known, CausetsColumn::Attribute, attribute)
            .and_then(|a| {
                // Make sure that, if it's an solitonId, it names an attribute.
                if let EvolvedNonValuePlace::SolitonId(e) = a {
                    if let Some(attr) = known.schema.attribute_for_entid(e) {
                        Place((a, Some(attr.value_type)))
                    } else {
                        Empty(EmptyBecause::InvalidAttributeSolitonId(e))
                    }
                } else {
                    Place((a, None))
                }
            })
    }

    pub(crate) fn make_evolved_value(&self,
                                     known: &Known,
                                     value_type: Option<ValueType>,
                                     value: PatternValuePlace) -> PlaceOrEmpty<EvolvedValuePlace> {
        use self::PlaceOrEmpty::*;
        match value {
            PatternValuePlace::Placeholder => Place(EvolvedValuePlace::Placeholder),
            PatternValuePlace::SolitonIdOrInteger(e) => {
                match value_type {
                    Some(ValueType::Ref) => Place(EvolvedValuePlace::SolitonId(e)),
                    Some(ValueType::Long) => Place(EvolvedValuePlace::Value(TypedValue::Long(e))),
                    Some(ValueType::Double) => Place(EvolvedValuePlace::Value((e as f64).into())),
                    Some(t) => Empty(EmptyBecause::ValueTypeMismatch(t, TypedValue::Long(e))),
                    None => Place(EvolvedValuePlace::SolitonIdOrInteger(e)),
                }
            },
            PatternValuePlace::CausetIdOrKeyword(kw) => {
                match value_type {
                    Some(ValueType::Ref) => {
                        // Resolve the causetid.
                        if let Some(solitonId) = known.schema.get_entid(&kw) {
                            Place(EvolvedValuePlace::SolitonId(solitonId.into()))
                        } else {
                            Empty(EmptyBecause::UnresolvedCausetId((&*kw).clone()))
                        }
                    },
                    Some(ValueType::Keyword) => {
                        Place(EvolvedValuePlace::Value(TypedValue::Keyword(kw)))
                    },
                    Some(t) => {
                        Empty(EmptyBecause::ValueTypeMismatch(t, TypedValue::Keyword(kw)))
                    },
                    None => {
                        Place(EvolvedValuePlace::CausetIdOrKeyword(kw))
                    },
                }
            },
            PatternValuePlace::Variable(var) => {
                // See if we have it!
                match self.bound_value(&var) {
                    None => Place(EvolvedValuePlace::Variable(var)),
                    Some(TypedValue::Ref(solitonId)) => {
                        if let Some(empty) = self.can_constrain_var_to_type(&var, ValueType::Ref) {
                            Empty(empty)
                        } else {
                            Place(EvolvedValuePlace::SolitonId(solitonId))
                        }
                    },
                    Some(val) => {
                        if let Some(empty) = self.can_constrain_var_to_type(&var, val.value_type()) {
                            Empty(empty)
                        } else {
                            Place(EvolvedValuePlace::Value(val))
                        }
                    },
                }
            },
            PatternValuePlace::Constant(nic) => {
                Place(EvolvedValuePlace::Value(into_typed_value(nic)))
            },
        }
    }

    pub(crate) fn make_evolved_pattern(&self, known: Known, pattern: Pattern) -> PlaceOrEmpty<EvolvedPattern> {
        let (e, a, v, causetx, source) = (pattern.instanton, pattern.attribute, pattern.value, pattern.causetx, pattern.source);
        use self::PlaceOrEmpty::*;
        match self.make_evolved_instanton(&known, e) {
            Empty(because) => Empty(because),
            Place(e) => {
                match self.make_evolved_attribute(&known, a) {
                    Empty(because) => Empty(because),
                    Place((a, value_type)) => {
                        match self.make_evolved_value(&known, value_type, v) {
                            Empty(because) => Empty(because),
                            Place(v) => {
                                match self.make_evolved_causecausetx(&known, causetx) {
                                    Empty(because) => Empty(because),
                                    Place(causetx) => {
                                        PlaceOrEmpty::Place(EvolvedPattern {
                                            source: source.unwrap_or(SrcVar::DefaultSrc),
                                            instanton: e,
                                            attribute: a,
                                            value: v,
                                            causetx: causetx,
                                        })
                                    },
                                }
                            },
                        }
                    },
                }
            },
        }
    }

    /// Re-examine the pattern to see if it can be specialized or is now known to fail.
    #[allow(unused_variables)]
    pub(crate) fn evolve_pattern(&mut self, known: Known, mut pattern: EvolvedPattern) -> PlaceOrEmpty<EvolvedPattern> {
        use self::PlaceOrEmpty::*;

        let mut new_instanton: Option<EvolvedNonValuePlace> = None;
        let mut new_value: Option<EvolvedValuePlace> = None;

        match &pattern.instanton {
            &EvolvedNonValuePlace::Variable(ref var) => {
                // See if we have it yet!
                match self.bound_value(&var) {
                    None => (),
                    Some(TypedValue::Ref(solitonId)) => {
                        new_instanton = Some(EvolvedNonValuePlace::SolitonId(solitonId));
                    },
                    Some(v) => {
                        return Empty(EmptyBecause::TypeMismatch {
                            var: var.clone(),
                            existing: self.known_type_set(&var),
                            desired: ValueTypeSet::of_one(ValueType::Ref),
                        });
                    },
                };
            },
            _ => (),
        }
        match &pattern.value {
            &EvolvedValuePlace::Variable(ref var) => {
                // See if we have it yet!
                match self.bound_value(&var) {
                    None => (),
                    Some(tv) => {
                        new_value = Some(EvolvedValuePlace::Value(tv.clone()));
                    },
                };
            },
            _ => (),
        }


        if let Some(e) = new_instanton {
            pattern.instanton = e;
        }
        if let Some(v) = new_value {
            pattern.value = v;
        }
        Place(pattern)
    }

    #[cfg(test)]
    pub(crate) fn apply_parsed_pattern(&mut self, known: Known, pattern: Pattern) {
        use self::PlaceOrEmpty::*;
        match self.make_evolved_pattern(known, pattern) {
            Empty(e) => self.mark_known_empty(e),
            Place(p) => self.apply_pattern(known, p),
        };
    }

    pub(crate) fn apply_pattern(&mut self, known: Known, pattern: EvolvedPattern) {
        // For now we only support the default source.
        if pattern.source != SrcVar::DefaultSrc {
            unimplemented!();
        }

        if self.attempt_cache_lookup(known, &pattern) {
            return;
        }

        if let Some(alias) = self.alias_table(known.schema, &pattern) {
            self.apply_pattern_clause_for_alias(known, &pattern, &alias);
            self.from.push(alias);
        } else {
            // We didn't determine a table, likely because there was a mismatch
            // between an attribute and a value.
            // We know we cannot return a result, so we short-circuit here.
            self.mark_known_empty(EmptyBecause::AttributeLookupFailed);
            return;
        }
    }
}

#[cfg(test)]
mod testing {
    use super::*;

    use std::collections::BTreeMap;
    use std::collections::BTreeSet;

    use embedded_promises::attribute::{
        Unique,
    };
    use embedded_promises::{
        Attribute,
        ValueTypeSet,
    };
    use einsteindb_embedded::{
        Schema,
    };

    use edbn::causetq::{
        Keyword,
        Variable,
    };

    use clauses::{
        CausetQInputs,
        add_attribute,
        associate_causetId,
        causetid,
    };

    use types::{
        Column,
        ColumnConstraint,
        CausetsTable,
        QualifiedAlias,
        CausetQValue,
        SourceAlias,
    };

    use {
        algebrize,
        parse_find_string,
    };

    fn alg(schema: &Schema, input: &str) -> ConjoiningClauses {
        let parsed = parse_find_string(input).expect("parse failed");
        let known = Known::for_schema(schema);
        algebrize(known, parsed).expect("algebrize failed").cc
    }

    #[test]
    fn test_unknown_causetId() {
        let mut cc = ConjoiningClauses::default();
        let schema = Schema::default();
        let known = Known::for_schema(&schema);

        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(Variable::from_valid_name("?x")),
            attribute: causetid("foo", "bar"),
            value: PatternValuePlace::Constant(NonIntegerConstant::Boolean(true)),
            causetx: PatternNonValuePlace::Placeholder,
        });

        assert!(cc.is_known_empty());
    }

    #[test]
    fn test_unknown_attribute() {
        let mut cc = ConjoiningClauses::default();
        let mut schema = Schema::default();

        associate_causetId(&mut schema, Keyword::namespaced("foo", "bar"), 99);

        let known = Known::for_schema(&schema);
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(Variable::from_valid_name("?x")),
            attribute: causetid("foo", "bar"),
            value: PatternValuePlace::Constant(NonIntegerConstant::Boolean(true)),
            causetx: PatternNonValuePlace::Placeholder,
        });

        assert!(cc.is_known_empty());
    }

    #[test]
    fn test_apply_simple_pattern() {
        let mut cc = ConjoiningClauses::default();
        let mut schema = Schema::default();

        associate_causetId(&mut schema, Keyword::namespaced("foo", "bar"), 99);
        add_attribute(&mut schema, 99, Attribute {
            value_type: ValueType::Boolean,
            ..Default::default()
        });

        let x = Variable::from_valid_name("?x");
        let known = Known::for_schema(&schema);
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(x.clone()),
            attribute: causetid("foo", "bar"),
            value: PatternValuePlace::Constant(NonIntegerConstant::Boolean(true)),
            causetx: PatternNonValuePlace::Placeholder,
        });

        // println!("{:#?}", cc);

        let d0_e = QualifiedAlias::new("Causets00".to_string(), CausetsColumn::Instanton);
        let d0_a = QualifiedAlias::new("Causets00".to_string(), CausetsColumn::Attribute);
        let d0_v = QualifiedAlias::new("Causets00".to_string(), CausetsColumn::Value);

        // After this, we know a lot of things:
        assert!(!cc.is_known_empty());
        assert_eq!(cc.from, vec![SourceAlias(CausetsTable::Causets, "Causets00".to_string())]);

        // ?x must be a ref.
        assert_eq!(cc.known_type(&x).unwrap(), ValueType::Ref);

        // ?x is bound to Causets0.e.
        assert_eq!(cc.column_bindings.get(&x).unwrap(), &vec![d0_e.clone()]);

        // Our 'where' clauses are two:
        // - Causets0.a = 99
        // - Causets0.v = true
        // No need for a type tag constraint, because the attribute is known.
        assert_eq!(cc.wheres, vec![
                   ColumnConstraint::Equals(d0_a, CausetQValue::SolitonId(99)),
                   ColumnConstraint::Equals(d0_v, CausetQValue::TypedValue(TypedValue::Boolean(true))),
        ].into());
    }

    #[test]
    fn test_apply_unattributed_pattern() {
        let mut cc = ConjoiningClauses::default();
        let schema = Schema::default();

        let x = Variable::from_valid_name("?x");
        let known = Known::for_schema(&schema);
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(x.clone()),
            attribute: PatternNonValuePlace::Placeholder,
            value: PatternValuePlace::Constant(NonIntegerConstant::Boolean(true)),
            causetx: PatternNonValuePlace::Placeholder,
        });

        // println!("{:#?}", cc);

        let d0_e = QualifiedAlias::new("Causets00".to_string(), CausetsColumn::Instanton);
        let d0_v = QualifiedAlias::new("Causets00".to_string(), CausetsColumn::Value);

        assert!(!cc.is_known_empty());
        assert_eq!(cc.from, vec![SourceAlias(CausetsTable::Causets, "Causets00".to_string())]);

        // ?x must be a ref.
        assert_eq!(cc.known_type(&x).unwrap(), ValueType::Ref);

        // ?x is bound to Causets0.e.
        assert_eq!(cc.column_bindings.get(&x).unwrap(), &vec![d0_e.clone()]);

        // Our 'where' clauses are two:
        // - Causets0.v = true
        // - Causets0.value_type_tag = boolean
        // TODO: implement expand_type_tags.
        assert_eq!(cc.wheres, vec![
                   ColumnConstraint::Equals(d0_v, CausetQValue::TypedValue(TypedValue::Boolean(true))),
                   ColumnConstraint::has_unit_type("Causets00".to_string(), ValueType::Boolean),
        ].into());
    }

    /// This test ensures that we do less work if we know the attribute thanks to a var lookup.
    #[test]
    fn test_apply_unattributed_but_bound_pattern_with_returned() {
        let mut cc = ConjoiningClauses::default();
        let mut schema = Schema::default();
        associate_causetId(&mut schema, Keyword::namespaced("foo", "bar"), 99);
        add_attribute(&mut schema, 99, Attribute {
            value_type: ValueType::Boolean,
            ..Default::default()
        });

        let x = Variable::from_valid_name("?x");
        let a = Variable::from_valid_name("?a");
        let v = Variable::from_valid_name("?v");

        cc.input_variables.insert(a.clone());
        cc.value_bindings.insert(a.clone(), TypedValue::typed_ns_keyword("foo", "bar"));
        let known = Known::for_schema(&schema);
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(x.clone()),
            attribute: PatternNonValuePlace::Variable(a.clone()),
            value: PatternValuePlace::Variable(v.clone()),
            causetx: PatternNonValuePlace::Placeholder,
        });

        // println!("{:#?}", cc);

        let d0_e = QualifiedAlias::new("Causets00".to_string(), CausetsColumn::Instanton);
        let d0_a = QualifiedAlias::new("Causets00".to_string(), CausetsColumn::Attribute);

        assert!(!cc.is_known_empty());
        assert_eq!(cc.from, vec![SourceAlias(CausetsTable::Causets, "Causets00".to_string())]);

        // ?x must be a ref, and ?v a boolean.
        assert_eq!(cc.known_type(&x), Some(ValueType::Ref));

        // We don't need to extract a type for ?v, because the attribute is known.
        assert!(!cc.extracted_types.contains_key(&v));
        assert_eq!(cc.known_type(&v), Some(ValueType::Boolean));

        // ?x is bound to Causets0.e.
        assert_eq!(cc.column_bindings.get(&x).unwrap(), &vec![d0_e.clone()]);
        assert_eq!(cc.wheres, vec![
                   ColumnConstraint::Equals(d0_a, CausetQValue::SolitonId(99)),
        ].into());
    }

    /// Queries that bind non-instanton values to instanton places can't return results.
    #[test]
    fn test_bind_the_wrong_thing() {
        let mut cc = ConjoiningClauses::default();
        let schema = Schema::default();

        let x = Variable::from_valid_name("?x");
        let a = Variable::from_valid_name("?a");
        let v = Variable::from_valid_name("?v");
        let hello = TypedValue::typed_string("hello");

        cc.input_variables.insert(a.clone());
        cc.value_bindings.insert(a.clone(), hello.clone());
        let known = Known::for_schema(&schema);
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(x.clone()),
            attribute: PatternNonValuePlace::Variable(a.clone()),
            value: PatternValuePlace::Variable(v.clone()),
            causetx: PatternNonValuePlace::Placeholder,
        });

        assert!(cc.is_known_empty());
        assert_eq!(cc.empty_because.unwrap(), EmptyBecause::InvalidBinding(Column::Fixed(CausetsColumn::Attribute), hello));
    }


    /// This test ensures that we causetq all_Causets if we're possibly retrieving a string.
    #[test]
    fn test_apply_unattributed_pattern_with_returned() {
        let mut cc = ConjoiningClauses::default();
        let schema = Schema::default();

        let x = Variable::from_valid_name("?x");
        let a = Variable::from_valid_name("?a");
        let v = Variable::from_valid_name("?v");
        let known = Known::for_schema(&schema);
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(x.clone()),
            attribute: PatternNonValuePlace::Variable(a.clone()),
            value: PatternValuePlace::Variable(v.clone()),
            causetx: PatternNonValuePlace::Placeholder,
        });

        // println!("{:#?}", cc);

        let d0_e = QualifiedAlias::new("all_Causets00".to_string(), CausetsColumn::Instanton);

        assert!(!cc.is_known_empty());
        assert_eq!(cc.from, vec![SourceAlias(CausetsTable::AllCausets, "all_Causets00".to_string())]);

        // ?x must be a ref.
        assert_eq!(cc.known_type(&x).unwrap(), ValueType::Ref);

        // ?x is bound to Causets0.e.
        assert_eq!(cc.column_bindings.get(&x).unwrap(), &vec![d0_e.clone()]);
        assert_eq!(cc.wheres, vec![].into());
    }

    /// This test ensures that we causetq all_Causets if we're looking for a string.
    #[test]
    fn test_apply_unattributed_pattern_with_string_value() {
        let mut cc = ConjoiningClauses::default();
        let schema = Schema::default();

        let x = Variable::from_valid_name("?x");
        let known = Known::for_schema(&schema);
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(x.clone()),
            attribute: PatternNonValuePlace::Placeholder,
            value: PatternValuePlace::Constant("hello".into()),
            causetx: PatternNonValuePlace::Placeholder,
        });

        // println!("{:#?}", cc);

        let d0_e = QualifiedAlias::new("all_Causets00".to_string(), CausetsColumn::Instanton);
        let d0_v = QualifiedAlias::new("all_Causets00".to_string(), CausetsColumn::Value);

        assert!(!cc.is_known_empty());
        assert_eq!(cc.from, vec![SourceAlias(CausetsTable::AllCausets, "all_Causets00".to_string())]);

        // ?x must be a ref.
        assert_eq!(cc.known_type(&x).unwrap(), ValueType::Ref);

        // ?x is bound to Causets0.e.
        assert_eq!(cc.column_bindings.get(&x).unwrap(), &vec![d0_e.clone()]);

        // Our 'where' clauses are two:
        // - Causets0.v = 'hello'
        // - Causets0.value_type_tag = string
        // TODO: implement expand_type_tags.
        assert_eq!(cc.wheres, vec![
                   ColumnConstraint::Equals(d0_v, CausetQValue::TypedValue(TypedValue::typed_string("hello"))),
                   ColumnConstraint::has_unit_type("all_Causets00".to_string(), ValueType::String),
        ].into());
    }

    #[test]
    fn test_apply_two_patterns() {
        let mut cc = ConjoiningClauses::default();
        let mut schema = Schema::default();

        associate_causetId(&mut schema, Keyword::namespaced("foo", "bar"), 99);
        associate_causetId(&mut schema, Keyword::namespaced("foo", "roz"), 98);
        add_attribute(&mut schema, 99, Attribute {
            value_type: ValueType::Boolean,
            ..Default::default()
        });
        add_attribute(&mut schema, 98, Attribute {
            value_type: ValueType::String,
            ..Default::default()
        });

        let x = Variable::from_valid_name("?x");
        let y = Variable::from_valid_name("?y");
        let known = Known::for_schema(&schema);
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(x.clone()),
            attribute: causetid("foo", "roz"),
            value: PatternValuePlace::Constant("idgoeshere".into()),
            causetx: PatternNonValuePlace::Placeholder,
        });
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(x.clone()),
            attribute: causetid("foo", "bar"),
            value: PatternValuePlace::Variable(y.clone()),
            causetx: PatternNonValuePlace::Placeholder,
        });

        // Finally, expand column bindings to get the overlaps for ?x.
        cc.expand_column_bindings();

        println!("{:#?}", cc);

        let d0_e = QualifiedAlias::new("Causets00".to_string(), CausetsColumn::Instanton);
        let d0_a = QualifiedAlias::new("Causets00".to_string(), CausetsColumn::Attribute);
        let d0_v = QualifiedAlias::new("Causets00".to_string(), CausetsColumn::Value);
        let d1_e = QualifiedAlias::new("Causets01".to_string(), CausetsColumn::Instanton);
        let d1_a = QualifiedAlias::new("Causets01".to_string(), CausetsColumn::Attribute);

        assert!(!cc.is_known_empty());
        assert_eq!(cc.from, vec![
                   SourceAlias(CausetsTable::Causets, "Causets00".to_string()),
                   SourceAlias(CausetsTable::Causets, "Causets01".to_string()),
        ]);

        // ?x must be a ref.
        assert_eq!(cc.known_type(&x).unwrap(), ValueType::Ref);

        // ?x is bound to Causets0.e and Causets1.e.
        assert_eq!(cc.column_bindings.get(&x).unwrap(),
                   &vec![
                       d0_e.clone(),
                       d1_e.clone(),
                   ]);

        // Our 'where' clauses are four:
        // - Causets0.a = 98 (:foo/roz)
        // - Causets0.v = "idgoeshere"
        // - Causets1.a = 99 (:foo/bar)
        // - Causets1.e = Causets0.e
        assert_eq!(cc.wheres, vec![
                   ColumnConstraint::Equals(d0_a, CausetQValue::SolitonId(98)),
                   ColumnConstraint::Equals(d0_v, CausetQValue::TypedValue(TypedValue::typed_string("idgoeshere"))),
                   ColumnConstraint::Equals(d1_a, CausetQValue::SolitonId(99)),
                   ColumnConstraint::Equals(d0_e, CausetQValue::Column(d1_e)),
        ].into());
    }

    #[test]
    fn test_value_bindings() {
        let mut schema = Schema::default();

        associate_causetId(&mut schema, Keyword::namespaced("foo", "bar"), 99);
        add_attribute(&mut schema, 99, Attribute {
            value_type: ValueType::Boolean,
            ..Default::default()
        });

        let x = Variable::from_valid_name("?x");
        let y = Variable::from_valid_name("?y");

        let b: BTreeMap<Variable, TypedValue> =
            vec![(y.clone(), TypedValue::Boolean(true))].into_iter().collect();
        let inputs = CausetQInputs::with_values(b);
        let variables: BTreeSet<Variable> = vec![Variable::from_valid_name("?y")].into_iter().collect();
        let mut cc = ConjoiningClauses::with_inputs(variables, inputs);

        let known = Known::for_schema(&schema);
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(x.clone()),
            attribute: causetid("foo", "bar"),
            value: PatternValuePlace::Variable(y.clone()),
            causetx: PatternNonValuePlace::Placeholder,
        });

        let d0_e = QualifiedAlias::new("Causets00".to_string(), CausetsColumn::Instanton);
        let d0_a = QualifiedAlias::new("Causets00".to_string(), CausetsColumn::Attribute);
        let d0_v = QualifiedAlias::new("Causets00".to_string(), CausetsColumn::Value);

        // ?y has been expanded into `true`.
        assert_eq!(cc.wheres, vec![
                   ColumnConstraint::Equals(d0_a, CausetQValue::SolitonId(99)),
                   ColumnConstraint::Equals(d0_v, CausetQValue::TypedValue(TypedValue::Boolean(true))),
        ].into());

        // There is no binding for ?y.
        assert!(!cc.column_bindings.contains_key(&y));

        // ?x is bound to the instanton.
        assert_eq!(cc.column_bindings.get(&x).unwrap(),
                   &vec![d0_e.clone()]);
    }

    #[test]
    /// Bind a value to a variable in a causetq where the type of the value disagrees with the type of
    /// the variable inferred from known attributes.
    fn test_value_bindings_type_disagreement() {
        let mut schema = Schema::default();

        associate_causetId(&mut schema, Keyword::namespaced("foo", "bar"), 99);
        add_attribute(&mut schema, 99, Attribute {
            value_type: ValueType::Boolean,
            ..Default::default()
        });

        let x = Variable::from_valid_name("?x");
        let y = Variable::from_valid_name("?y");

        let b: BTreeMap<Variable, TypedValue> =
            vec![(y.clone(), TypedValue::Long(42))].into_iter().collect();
        let inputs = CausetQInputs::with_values(b);
        let variables: BTreeSet<Variable> = vec![Variable::from_valid_name("?y")].into_iter().collect();
        let mut cc = ConjoiningClauses::with_inputs(variables, inputs);

        let known = Known::for_schema(&schema);
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(x.clone()),
            attribute: causetid("foo", "bar"),
            value: PatternValuePlace::Variable(y.clone()),
            causetx: PatternNonValuePlace::Placeholder,
        });

        // The type of the provided binding doesn't match the type of the attribute.
        assert!(cc.is_known_empty());
    }

    #[test]
    /// Bind a non-textual value to a variable in a causetq where the variable is used as the value
    /// of a fulltext-valued attribute.
    fn test_fulltext_type_disagreement() {
        let mut schema = Schema::default();

        associate_causetId(&mut schema, Keyword::namespaced("foo", "bar"), 99);
        add_attribute(&mut schema, 99, Attribute {
            value_type: ValueType::String,
            index: true,
            fulltext: true,
            ..Default::default()
        });

        let x = Variable::from_valid_name("?x");
        let y = Variable::from_valid_name("?y");

        let b: BTreeMap<Variable, TypedValue> =
            vec![(y.clone(), TypedValue::Long(42))].into_iter().collect();
        let inputs = CausetQInputs::with_values(b);
        let variables: BTreeSet<Variable> = vec![Variable::from_valid_name("?y")].into_iter().collect();
        let mut cc = ConjoiningClauses::with_inputs(variables, inputs);

        let known = Known::for_schema(&schema);
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(x.clone()),
            attribute: causetid("foo", "bar"),
            value: PatternValuePlace::Variable(y.clone()),
            causetx: PatternNonValuePlace::Placeholder,
        });

        // The type of the provided binding doesn't match the type of the attribute.
        assert!(cc.is_known_empty());
    }

    #[test]
    /// Apply two patterns with differently typed attributes, but sharing a variable in the value
    /// place. No value can bind to a variable and match both types, so the CC is known to return
    /// no results.
    fn test_apply_two_conflicting_known_patterns() {
        let mut cc = ConjoiningClauses::default();
        let mut schema = Schema::default();

        associate_causetId(&mut schema, Keyword::namespaced("foo", "bar"), 99);
        associate_causetId(&mut schema, Keyword::namespaced("foo", "roz"), 98);
        add_attribute(&mut schema, 99, Attribute {
            value_type: ValueType::Boolean,
            ..Default::default()
        });
        add_attribute(&mut schema, 98, Attribute {
            value_type: ValueType::String,
            unique: Some(Unique::CausetIdity),
            ..Default::default()
        });

        let x = Variable::from_valid_name("?x");
        let y = Variable::from_valid_name("?y");
        let known = Known::for_schema(&schema);
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(x.clone()),
            attribute: causetid("foo", "roz"),
            value: PatternValuePlace::Variable(y.clone()),
            causetx: PatternNonValuePlace::Placeholder,
        });
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(x.clone()),
            attribute: causetid("foo", "bar"),
            value: PatternValuePlace::Variable(y.clone()),
            causetx: PatternNonValuePlace::Placeholder,
        });

        // Finally, expand column bindings to get the overlaps for ?x.
        cc.expand_column_bindings();

        assert!(cc.is_known_empty());
        assert_eq!(cc.empty_because.unwrap(),
                   EmptyBecause::TypeMismatch {
                       var: y.clone(),
                       existing: ValueTypeSet::of_one(ValueType::String),
                       desired: ValueTypeSet::of_one(ValueType::Boolean),
                   });
    }

    #[test]
    #[should_panic(expected = "assertion failed: cc.is_known_empty()")]
    /// This test needs range inference in order to succeed: we must deduce that ?y must
    /// simultaneously be a boolean-valued attribute and a ref-valued attribute, and thus
    /// the CC can never return results.
    fn test_apply_two_implicitly_conflicting_patterns() {
        let mut cc = ConjoiningClauses::default();
        let schema = Schema::default();

        // [:find ?x :where
        //  [?x ?y true]
        //  [?z ?y ?x]]
        let x = Variable::from_valid_name("?x");
        let y = Variable::from_valid_name("?y");
        let z = Variable::from_valid_name("?z");
        let known = Known::for_schema(&schema);
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(x.clone()),
            attribute: PatternNonValuePlace::Variable(y.clone()),
            value: PatternValuePlace::Constant(NonIntegerConstant::Boolean(true)),
            causetx: PatternNonValuePlace::Placeholder,
        });
        cc.apply_parsed_pattern(known, Pattern {
            source: None,
            instanton: PatternNonValuePlace::Variable(z.clone()),
            attribute: PatternNonValuePlace::Variable(y.clone()),
            value: PatternValuePlace::Variable(x.clone()),
            causetx: PatternNonValuePlace::Placeholder,
        });

        // Finally, expand column bindings to get the overlaps for ?x.
        cc.expand_column_bindings();

        assert!(cc.is_known_empty());
        assert_eq!(cc.empty_because.unwrap(),
                   EmptyBecause::TypeMismatch {
                       var: x.clone(),
                       existing: ValueTypeSet::of_one(ValueType::Ref),
                       desired: ValueTypeSet::of_one(ValueType::Boolean),
                   });
    }

    #[test]
    fn ensure_extracted_types_is_cleared() {
        let causetq = r#"[:find ?e ?v :where [_ _ ?v] [?e :foo/bar ?v]]"#;
        let mut schema = Schema::default();
        associate_causetId(&mut schema, Keyword::namespaced("foo", "bar"), 99);
        add_attribute(&mut schema, 99, Attribute {
            value_type: ValueType::Boolean,
            ..Default::default()
        });
        let e = Variable::from_valid_name("?e");
        let v = Variable::from_valid_name("?v");
        let cc = alg(&schema, causetq);
        assert_eq!(cc.known_types.get(&e), Some(&ValueTypeSet::of_one(ValueType::Ref)));
        assert_eq!(cc.known_types.get(&v), Some(&ValueTypeSet::of_one(ValueType::Boolean)));
        assert!(!cc.extracted_types.contains_key(&e));
        assert!(!cc.extracted_types.contains_key(&v));
    }
}
