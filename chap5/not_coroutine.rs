use std::fs::OpenOptions;
use std::io::{Write, self};
use std::time::Instant;

use rand::RngExt;

fn append_num_to_file(n: i32) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open("randnum.txt")?;
    writeln!(file, "{}", n)?;
    Ok(())
}

fn main() -> io::Result<()> {
    let mut rng = rand::rng();
    let numbers: Vec<i32> = (0..200000).map(|_| rng.random()).collect();

    let start = Instant::now();
    for &number in &numbers {
        if let Err(e) = append_num_to_file(number) {
            eprintln!("Failed to write to file: {}", e);
        }
    }
    let duration = start.elapsed();

    println!("Time elapsed in file operations is: {:?}", duration);
    Ok(())
}