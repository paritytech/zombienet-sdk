use std::{collections::HashMap, num::ParseFloatError};

use pest::Parser;
use pest_derive::Parser;

/// An error at parsing level.
#[derive(thiserror::Error, Debug)]
pub enum ParserError {
    #[error("error parsing input")]
    ParseError(Box<pest::error::Error<Rule>>),
    #[error("root node should be valid: {0}")]
    ParseRootNodeError(String),
    #[error("can't cast metric value as f64: {0}")]
    CastValueError(#[from] ParseFloatError),
}

// This include forces recompiling this source file if the grammar file changes.
// Uncomment it when doing changes to the .pest file
const _GRAMMAR: &str = include_str!("grammar.pest");

#[derive(Parser)]
#[grammar = "grammar.pest"]
pub struct MetricsParser;

pub type MetricMap = HashMap<String, f64>;

pub fn parse(input: &str) -> Result<MetricMap, ParserError> {
    let mut metric_map: MetricMap = Default::default();
    let mut pairs = MetricsParser::parse(Rule::statement, input)
        .map_err(|e| ParserError::ParseError(Box::new(e)))?;

    let root = pairs
        .next()
        .ok_or(ParserError::ParseRootNodeError(pairs.as_str().to_string()))?;
    for token in root.into_inner() {
        // match token.as_rule() {
        //     Rule::block => {
        if token.as_rule() == Rule::block {
            let inner = token.into_inner();
            for value in inner {
                match value.as_rule() {
                    Rule::genericomment | Rule::typexpr | Rule::helpexpr => {
                        // don't need to collect comments/types/helpers blocks.
                        continue;
                    },
                    Rule::promstmt => {
                        let mut key: &str = "";
                        let mut labels: Vec<(&str, &str)> = Vec::new();
                        let mut val: f64 = 0_f64;
                        for v in value.clone().into_inner() {
                            match &v.as_rule() {
                                Rule::key => {
                                    key = v.as_span().as_str();
                                },
                                Rule::NaN | Rule::posInf | Rule::negInf => {
                                    // noop (not used in substrate metrics)
                                },
                                Rule::number => {
                                    val = v.as_span().as_str().parse::<f64>()?;
                                },
                                Rule::labels => {
                                    // SAFETY: use unwrap should be safe since we are just
                                    // walking the parser struct and if are matching a label
                                    // should have a key/vals
                                    for p in v.into_inner() {
                                        let mut inner = p.into_inner();
                                        let key = inner.next().unwrap().as_span().as_str();
                                        let value = inner
                                            .next()
                                            .unwrap()
                                            .into_inner()
                                            .next()
                                            .unwrap()
                                            .as_span()
                                            .as_str();

                                        labels.push((key, value));
                                    }
                                },
                                _ => {
                                    todo!("not implemented");
                                },
                            }
                        }

                        // we should store to make it compatible with zombienet v1:
                        // key_without_prefix
                        // key_without_prefix_and_without_chain
                        // key_with_prefix_with_chain
                        // key_with_prefix_and_without_chain
                        let key_with_out_prefix =
                            key.split('_').collect::<Vec<&str>>()[1..].join("_");
                        let (labels_without_chain, labels_with_chain) =
                            labels.iter().fold((vec![], vec![]), |mut acc, item| {
                                if item.0.eq("chain") {
                                    acc.1.push(format!("{}=\"{}\"", item.0, item.1));
                                } else {
                                    acc.0.push(format!("{}=\"{}\"", item.0, item.1));
                                    acc.1.push(format!("{}=\"{}\"", item.0, item.1));
                                }
                                acc
                            });

                        let labels_with_chain_str = if labels_with_chain.is_empty() {
                            String::from("")
                        } else {
                            format!("{{{}}}", labels_with_chain.join(","))
                        };

                        let labels_without_chain_str = if labels_without_chain.is_empty() {
                            String::from("")
                        } else {
                            format!("{{{}}}", labels_without_chain.join(","))
                        };

                        metric_map.insert(format!("{}{}", key, labels_without_chain_str), val);
                        metric_map.insert(
                            format!("{}{}", key_with_out_prefix, labels_without_chain_str),
                            val,
                        );
                        metric_map.insert(format!("{}{}", key, labels_with_chain_str), val);
                        metric_map.insert(
                            format!("{}{}", key_with_out_prefix, labels_with_chain_str),
                            val,
                        );
                    },
                    _ => {},
                }
            }
        }
    }

    Ok(metric_map)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn parse_metrics_works() {
        let metrics_raw = fs::read_to_string("./testing/metrics.txt").unwrap();
        let metrics = parse(&metrics_raw).unwrap();

        // full key
        assert_eq!(
            metrics
                .get("polkadot_node_is_active_validator{chain=\"rococo_local_testnet\"}")
                .unwrap(),
            &1_f64
        );
        // with prefix and no chain
        assert_eq!(
            metrics.get("polkadot_node_is_active_validator").unwrap(),
            &1_f64
        );
        // no prefix with chain
        assert_eq!(
            metrics
                .get("node_is_active_validator{chain=\"rococo_local_testnet\"}")
                .unwrap(),
            &1_f64
        );
        // no prefix without chain
        assert_eq!(metrics.get("node_is_active_validator").unwrap(), &1_f64);
    }

    #[test]
    fn parse_invalid_metrics_str_should_fail() {
        let metrics_raw = r#"
        # HELP polkadot_node_is_active_validator Tracks if the validator is in the active set. Updates at session boundary.
        # TYPE polkadot_node_is_active_validator gauge
        polkadot_node_is_active_validator{chain=} 1
        "#;

        let metrics = parse(metrics_raw);
        assert!(metrics.is_err());
        assert!(matches!(metrics, Err(ParserError::ParseError(_))));
    }
}
