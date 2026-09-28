//! Prints how a model scores related and unrelated pairs, to choose its cutoff.
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = Path::new(&args[0]);
    let pairs = [
        (
            "rental lease",
            "The landlord and tenant agree to the monthly rent and the term of this tenancy.",
        ),
        (
            "script that backs up photos",
            "rsync -av ~/Pictures nas:/photos",
        ),
        (
            "授權條款",
            "Licensed under the Apache License, Version 2.0; you may not use this file except in compliance.",
        ),
        ("invoice", "Amount due: $1,249.00. Payment terms: net 30."),
        ("rental lease", "rsync -av ~/Pictures nas:/photos"),
        (
            "script that backs up photos",
            "The landlord and tenant agree to the monthly rent.",
        ),
        ("invoice", "Chapter 3: the history of the Roman empire."),
        ("授權條款", "Lunch with Sam on Friday at noon."),
    ];
    for model in &args[1..] {
        let mut m = sonar_models::load(
            model,
            dir,
            sonar_models::Load {
                download: true,
                threads: 4,
            },
        )?;
        println!("== {model}");
        for (q, p) in pairs {
            let a = m.query(q)?;
            let b = m.passages(&[p.to_owned()])?.remove(0);
            let norm: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
            println!(
                "  {:.3} (|p|={norm:.2})  {q}  <->  {}",
                a.iter().zip(&b).map(|(x, y)| x * y).sum::<f32>(),
                &p[..p.len().min(40)]
            );
        }
    }
    Ok(())
}
