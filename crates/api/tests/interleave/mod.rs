//! Interleaving tests (build rule 8). Each Kafka topic has one partition, so a consumer sees
//! each topic in its own order, but how the topics interleave is the consumer's luck: a
//! consumer of several topics must give the same output whichever interleaving it gets.

use det::{Rng, SeededRng};

/// Interleavings of `topics` that keep each topic's records in their order, each named: the two
/// extremes, every topic read whole in turn first to last and then last to first, and `random`
/// more drawn from seeds 1 to `random`.
pub fn interleavings<T>(topics: &[Vec<T>], random: u64) -> Vec<(String, Vec<&T>)> {
    let mut orders = vec![
        (
            "each topic whole, first to last".to_string(),
            topics.iter().flatten().collect(),
        ),
        (
            "each topic whole, last to first".to_string(),
            topics.iter().rev().flatten().collect(),
        ),
    ];
    for seed in 1..=random {
        orders.push((format!("random, seed {seed}"), drawn(topics, seed)));
    }
    orders
}

/// Each next record is drawn evenly from all the records left, so every interleaving is as
/// likely as any other.
fn drawn<T>(topics: &[Vec<T>], seed: u64) -> Vec<&T> {
    let mut rng = SeededRng::from_seed(seed);
    let mut next = vec![0; topics.len()];
    let mut left: usize = topics.iter().map(Vec::len).sum();
    let mut order = Vec::with_capacity(left);
    while left > 0 {
        let mut draw = (rng.next_u64() % left as u64) as usize;
        for (topic, records) in topics.iter().enumerate() {
            let remaining = records.len() - next[topic];
            if draw < remaining {
                order.push(&records[next[topic]]);
                next[topic] += 1;
                break;
            }
            draw -= remaining;
        }
        left -= 1;
    }
    order
}

/// Asserts `consume` gives byte-identical output for every one of `orders`, naming the first
/// interleaving that differs and its first differing line, and returns that output, for the test
/// to check it isn't empty.
pub fn assert_same_output<T>(
    orders: &[(String, Vec<&T>)],
    mut consume: impl FnMut(&[&T]) -> String,
) -> String {
    let (first, rest) = orders.split_first().expect("at least one interleaving");
    let expected = consume(&first.1);
    for (name, order) in rest {
        let output = consume(order);
        if output == expected {
            continue;
        }
        let want: Vec<&str> = expected.lines().collect();
        let got: Vec<&str> = output.lines().collect();
        let line = (0..want.len().max(got.len()))
            .find(|&i| want.get(i) != got.get(i))
            .unwrap_or(want.len());
        let end = "<end of output>";
        let (want, got) = around_difference(
            want.get(line).unwrap_or(&end),
            got.get(line).unwrap_or(&end),
        );
        panic!(
            "the output depends on how the topics interleave: line {} differs\n{}: {want}\n{name}: {got}",
            line + 1,
            first.0,
        );
    }
    expected
}

/// Up to 300 bytes of each line, from a little before where they first differ: a whole feed
/// is one line.
fn around_difference<'a>(want: &'a str, got: &'a str) -> (&'a str, &'a str) {
    let same = want
        .bytes()
        .zip(got.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let window = |line: &'a str| {
        let start = line.floor_char_boundary(same.saturating_sub(60));
        &line[start..line.floor_char_boundary(start + 300)]
    };
    (window(want), window(got))
}

#[test]
fn interleavings_keep_each_topics_order_and_draw_the_same_for_a_seed() {
    let topics = vec![vec!["a1", "a2", "a3"], vec!["b1", "b2", "b3"]];
    let orders: Vec<(String, String)> = interleavings(&topics, 3)
        .into_iter()
        .map(|(name, order)| {
            let order: Vec<&str> = order.into_iter().copied().collect();
            (name, order.join(" "))
        })
        .collect();
    let orders: Vec<(&str, &str)> = orders
        .iter()
        .map(|(name, order)| (name.as_str(), order.as_str()))
        .collect();
    assert_eq!(
        orders,
        [
            ("each topic whole, first to last", "a1 a2 a3 b1 b2 b3"),
            ("each topic whole, last to first", "b1 b2 b3 a1 a2 a3"),
            ("random, seed 1", "a1 a2 b1 a3 b2 b3"),
            ("random, seed 2", "a1 b1 a2 b2 b3 a3"),
            ("random, seed 3", "b1 b2 a1 b3 a2 a3"),
        ]
    );
}

#[test]
#[should_panic(expected = "the output depends on how the topics interleave: line 1 differs")]
fn a_consumer_whose_output_follows_the_interleaving_fails() {
    let topics = vec![vec![1, 2], vec![10]];
    assert_same_output(&interleavings(&topics, 0), |records| {
        records.iter().map(|r| format!("{r}\n")).collect()
    });
}
