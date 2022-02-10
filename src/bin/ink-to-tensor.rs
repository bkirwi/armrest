use armrest::ink::Ink;
use armrest::ml::{Bezier, Input, ModelInput, Spline};
use itertools::Itertools;
use std::io;
use std::io::prelude::*;

const MAX_INK: usize = 10000;

fn main() {
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    let args: Vec<_> = std::env::args().skip(1).collect();

    while let Some(Ok(line)) = lines.next() {
        let mut halves = line.split('\t');
        let expected = halves.next().unwrap();
        let points = halves.next().unwrap();

        let ink = Ink::from_string(points);
        let mut buffer = vec![0f32; MAX_INK];

        let (steps, size) = if args[0] == "bezier" {
            (
                ModelInput::<Bezier>::write_to(&ink, &mut buffer),
                Bezier::WIDTH,
            )
        } else {
            (
                ModelInput::<Spline>::write_to(&ink, &mut buffer),
                Spline::WIDTH,
            )
        };
        assert!(
            steps * size <= MAX_INK,
            "Error... longest ink was longer than expected!"
        );
        let tensor = buffer
            .chunks_exact(size)
            .take(steps)
            .map(|c| c.iter().map(|f| format!("{:.4}", f)).join(" "))
            .join(",");
        println!("{}\t{}", expected, tensor);
    }
}
