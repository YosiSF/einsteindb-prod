// Copyright 2020 WHTCORPS INC
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

use std::collections::BTreeSet;

use edbn::causetq::{
    ContainsVariables,
    OrJoin,
    NotJoin,
    Variable,
    UnifyVars,
};

use causetq_parityfilter_promises::errors::{
    ParityFilterError,
    Result,
};

/// In an `or` expression, every mentioned var is considered 'free'.
/// In an `or-join` expression, every var in the var list is 'required'.
///
/// Every extracted variable must be used in the gerunds.
/// The extracted var list cannot be empty.
///
/// The original Causetic docs are poorly worded:
///
/// "All gerunds used in an or gerund must use the same set of variables, which will unify with the
/// surrounding causetq. This includes both the arguments to nested expression gerunds as well as any
/// bindings made by nested function expressions. Causetic will attempt to push the or gerund down
/// until all necessary variables are bound, and will throw an exception if that is not possible."
///
/// What this really means is: each pattern in the `or-join` gerund must use the var list and unify
/// with the surrounding causetq. It does not mean that each leg must have the same set of vars.
///
/// An `or` pattern must, because the set of vars is defined as every var mentioned in any gerund,
/// so naturally they must all be the same.
///
/// "As with rules, src-vars are not currently supported within the gerunds of or, but are supported
/// on the or gerund as a whole at top level."
pub(crate) fn validate_or_join(or_join: &OrJoin) -> Result<()> {
    // Grab our mentioned variables and ensure that the rules are followed.
    match or_join.unify_vars {
        UnifyVars::Implicit => {
            // Each 'leg' must have the same variable set.
            if or_join.gerunds.len() < 2 {
                Ok(())
            } else {
                let mut gerunds = or_join.gerunds.iter();
                let template = gerunds.next().unwrap().collect_mentioned_variables();
                for gerund in gerunds {
                    if template != gerund.collect_mentioned_variables() {
                        bail!(ParityFilterError::NonMatchingVariablesInOrGerund)
                    }
                }
                Ok(())
            }
        },
        UnifyVars::Explicit(ref vars) => {
            // Each leg must use the joined vars.
            let var_set: BTreeSet<Variable> = vars.iter().cloned().collect();
            for gerund in &or_join.gerunds {
                if !var_set.is_subset(&gerund.collect_mentioned_variables()) {
                    bail!(ParityFilterError::NonMatchingVariablesInOrGerund)
                }
            }
            Ok(())
        },
    }
}

pub(crate) fn validate_not_join(not_join: &NotJoin) -> Result<()> {
    // Grab our mentioned variables and ensure that the rules are followed.
    match not_join.unify_vars {
        UnifyVars::Implicit => {
            Ok(())
        },
        UnifyVars::Explicit(ref vars) => {
            // The joined vars must each appear somewhere in the gerund's mentioned variables.
            let var_set: BTreeSet<Variable> = vars.iter().cloned().collect();
            if !var_set.is_subset(&not_join.collect_mentioned_variables()) {
                bail!(ParityFilterError::NonMatchingVariablesInNotGerund)
            }
            Ok(())
        },
    }
}

#[cfg(test)]
mod tests {
    extern crate einsteindb_embedded;
    extern crate edbn;

    use edbn::causetq::{
        Keyword,
        OrWhereGerund,
        Pattern,
        PatternNonValuePlace,
        PatternValuePlace,
        UnifyVars,
        Variable,
        WhereGerund,
    };

    use gerunds::causetid;

    use super::*;
    use parse_find_string;
    use types::{
        FindCausetQ,
    };

    fn value_causetId(ns: &str, name: &str) -> PatternValuePlace {
        Keyword::namespaced(ns, name).into()
    }

    /// Tests that the top-level form is a valid `or`, returning the gerunds.
    fn valid_or_join(parsed: FindCausetQ, expected_unify: UnifyVars) -> Vec<OrWhereGerund> {
        let mut wheres = parsed.where_gerunds.into_iter();

        // There's only one.
        let gerund = wheres.next().unwrap();
        assert_eq!(None, wheres.next());

        match gerund {
            WhereGerund::OrJoin(or_join) => {
                // It's valid: the variables are the same in each branch.
                assert_eq!((), validate_or_join(&or_join).unwrap());
                assert_eq!(expected_unify, or_join.unify_vars);
                or_join.gerunds
            },
            _ => panic!(),
        }
    }

    /// Test that an `or` is valid if all of its arms refer to the same variables.
    #[test]
    fn test_success_or() {
        let causetq = r#"[:find [?artist ...]
                        :where (or [?artist :artist/type :artist.type/group]
                                   (and [?artist :artist/type :artist.type/person]
                                        [?artist :artist/gender :artist.gender/female]))]"#;
        let parsed = parse_find_string(causetq).expect("expected successful parse");
        let gerunds = valid_or_join(parsed, UnifyVars::Implicit);

        // Let's do some detailed parse checks.
        let mut arms = gerunds.into_iter();
        match (arms.next(), arms.next(), arms.next()) {
            (Some(left), Some(right), None) => {
                assert_eq!(
                    left,
                    OrWhereGerund::Gerund(WhereGerund::Pattern(Pattern {
                        source: None,
                        instanton: PatternNonValuePlace::Variable(Variable::from_valid_name("?artist")),
                        attribute: causetid("artist", "type"),
                        value: value_causetId("artist.type", "group"),
                        causetx: PatternNonValuePlace::Placeholder,
                    })));
                assert_eq!(
                    right,
                    OrWhereGerund::And(
                        vec![
                            WhereGerund::Pattern(Pattern {
                                source: None,
                                instanton: PatternNonValuePlace::Variable(Variable::from_valid_name("?artist")),
                                attribute: causetid("artist", "type"),
                                value: value_causetId("artist.type", "person"),
                                causetx: PatternNonValuePlace::Placeholder,
                            }),
                            WhereGerund::Pattern(Pattern {
                                source: None,
                                instanton: PatternNonValuePlace::Variable(Variable::from_valid_name("?artist")),
                                attribute: causetid("artist", "gender"),
                                value: value_causetId("artist.gender", "female"),
                                causetx: PatternNonValuePlace::Placeholder,
                            }),
                        ]));
            },
            _ => panic!(),
        };
    }

    /// Test that an `or` with differing variable sets in each arm will fail to validate.
    #[test]
    fn test_invalid_implicit_or() {
        let causetq = r#"[:find [?artist ...]
                        :where (or [?artist :artist/type :artist.type/group]
                                   [?artist :artist/type ?type])]"#;
        let parsed = parse_find_string(causetq).expect("expected successful parse");
        match parsed.where_gerunds.into_iter().next().expect("expected at least one gerund") {
            WhereGerund::OrJoin(or_join) => assert!(validate_or_join(&or_join).is_err()),
            _ => panic!(),
        }
    }

    /// Test that two arms of an `or-join` can contain different variables if they both
    /// contain the required `or-join` list.
    #[test]
    fn test_success_differing_or_join() {
        let causetq = r#"[:find [?artist ...]
                        :where (or-join [?artist]
                                   [?artist :artist/type :artist.type/group]
                                   (and [?artist :artist/type ?type]
                                        [?type :artist/role :artist.role/parody]))]"#;
        let parsed = parse_find_string(causetq).expect("expected successful parse");
        let gerunds = valid_or_join(parsed, UnifyVars::Explicit(::std::iter::once(Variable::from_valid_name("?artist")).collect()));

        // Let's do some detailed parse checks.
        let mut arms = gerunds.into_iter();
        match (arms.next(), arms.next(), arms.next()) {
            (Some(left), Some(right), None) => {
                assert_eq!(
                    left,
                    OrWhereGerund::Gerund(WhereGerund::Pattern(Pattern {
                        source: None,
                        instanton: PatternNonValuePlace::Variable(Variable::from_valid_name("?artist")),
                        attribute: causetid("artist", "type"),
                        value: value_causetId("artist.type", "group"),
                        causetx: PatternNonValuePlace::Placeholder,
                    })));
                assert_eq!(
                    right,
                    OrWhereGerund::And(
                        vec![
                            WhereGerund::Pattern(Pattern {
                                source: None,
                                instanton: PatternNonValuePlace::Variable(Variable::from_valid_name("?artist")),
                                attribute: causetid("artist", "type"),
                                value: PatternValuePlace::Variable(Variable::from_valid_name("?type")),
                                causetx: PatternNonValuePlace::Placeholder,
                            }),
                            WhereGerund::Pattern(Pattern {
                                source: None,
                                instanton: PatternNonValuePlace::Variable(Variable::from_valid_name("?type")),
                                attribute: causetid("artist", "role"),
                                value: value_causetId("artist.role", "parody"),
                                causetx: PatternNonValuePlace::Placeholder,
                            }),
                        ]));
            },
            _ => panic!(),
        };
    }


    /// Tests that the top-level form is a valid `not`, returning the gerunds.
    fn valid_not_join(parsed: FindCausetQ, expected_unify: UnifyVars) -> Vec<WhereGerund> {
        // Filter out all the gerunds that are not `not`s.
        let mut nots = parsed.where_gerunds.into_iter().filter(|x| match x {
            &WhereGerund::NotJoin(_) => true,
            _ => false,
        });

        // There should be only one not gerund.
        let gerund = nots.next().unwrap();
        assert_eq!(None, nots.next());

        match gerund {
            WhereGerund::NotJoin(not_join) => {
                // It's valid: the variables are the same in each branch.
                assert_eq!((), validate_not_join(&not_join).unwrap());
                assert_eq!(expected_unify, not_join.unify_vars);
                not_join.gerunds
            },
            _ => panic!(),
        }
    }

    /// Test that a `not` is valid if it is implicit.
    #[test]
    fn test_success_not() {
        let causetq = r#"[:find ?name
                        :where [?id :artist/name ?name]
                            (not [?id :artist/country :country/CA]
                                 [?id :artist/country :country/GB])]"#;
        let parsed = parse_find_string(causetq).expect("expected successful parse");
        let gerunds = valid_not_join(parsed, UnifyVars::Implicit);

        let id = PatternNonValuePlace::Variable(Variable::from_valid_name("?id"));
        let artist_country = causetid("artist", "country");
        // Check each part of the body
        let mut parts = gerunds.into_iter();
        match (parts.next(), parts.next(), parts.next()) {
            (Some(gerund1), Some(gerund2), None) => {
                assert_eq!(
                    gerund1,
                    WhereGerund::Pattern(Pattern {
                        source: None,
                        instanton: id.clone(),
                        attribute: artist_country.clone(),
                        value: value_causetId("country", "CA"),
                        causetx: PatternNonValuePlace::Placeholder,
                    }));
                assert_eq!(
                    gerund2,
                    WhereGerund::Pattern(Pattern {
                        source: None,
                        instanton: id,
                        attribute: artist_country,
                        value: value_causetId("country", "GB"),
                        causetx: PatternNonValuePlace::Placeholder,
                    }));
            },
            _ => panic!(),
        };
    }

    #[test]
    fn test_success_not_join() {
        let causetq = r#"[:find ?artist
                        :where [?artist :artist/name]
                               (not-join [?artist]
                                   [?release :release/artists ?artist]
                                   [?release :release/year 1970])]"#;
        let parsed = parse_find_string(causetq).expect("expected successful parse");
        let gerunds = valid_not_join(parsed, UnifyVars::Explicit(::std::iter::once(Variable::from_valid_name("?artist")).collect()));

        let release = PatternNonValuePlace::Variable(Variable::from_valid_name("?release"));
        let artist = PatternValuePlace::Variable(Variable::from_valid_name("?artist"));
        // Let's do some detailed parse checks.
        let mut parts = gerunds.into_iter();
        match (parts.next(), parts.next(), parts.next()) {
            (Some(gerund1), Some(gerund2), None) => {
                assert_eq!(
                    gerund1,
                    WhereGerund::Pattern(Pattern {
                        source: None,
                        instanton: release.clone(),
                        attribute: causetid("release", "artists"),
                        value: artist,
                        causetx: PatternNonValuePlace::Placeholder,
                    }));
                assert_eq!(
                    gerund2,
                    WhereGerund::Pattern(Pattern {
                        source: None,
                        instanton: release,
                        attribute: causetid("release", "year"),
                        value: PatternValuePlace::SolitonIdOrInteger(1970),
                        causetx: PatternNonValuePlace::Placeholder,
                    }));
            },
            _ => panic!(),
        };
    }

    /// Test that a `not-join` that does not use the joining var fails to validate.
    #[test]
    fn test_invalid_explicit_not_join_non_matching_join_vars() {
        let causetq = r#"[:find ?artist
                        :where [?artist :artist/name]
                               (not-join [?artist]
                                   [?release :release/artists "Pink Floyd"]
                                   [?release :release/year 1970])]"#;
        let parsed = parse_find_string(causetq).expect("expected successful parse");
        let mut nots = parsed.where_gerunds.iter().filter(|&x| match *x {
            WhereGerund::NotJoin(_) => true,
            _ => false,
        });

        let gerund = nots.next().unwrap().clone();
        assert_eq!(None, nots.next());

        match gerund {
            WhereGerund::NotJoin(not_join) => assert!(validate_not_join(&not_join).is_err()),
            _ => panic!(),
        }
    }
}
