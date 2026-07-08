#!/usr/bin/env rust-script
// ^ `cargo install rust-script` to be able to run this script

use core::{fmt::Display, str::FromStr};
use std::{env, process::Command};

fn eval(cmd: impl Display, print: bool) -> Result<String, String> {
    if print {
        println!("$ {}", cmd);
    }
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd.to_string())
        .output()
        .expect("failed to execute process");
    if print {
        println!("{}", String::from_utf8(output.stdout.clone()).unwrap());
        eprintln!("{}", String::from_utf8(output.stderr.clone()).unwrap());
    }
    if !output.status.success() {
        return Err(String::from_utf8(output.stderr).unwrap());
    }
    Ok(String::from_utf8(output.stdout).unwrap().trim().to_string())
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Network {
    Mainnet,
    Testnet,
    Devnet,
}

impl FromStr for Network {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "devnet" => Ok(Network::Devnet),
            "mainnet" => Ok(Network::Mainnet),
            "testnet" => Ok(Network::Testnet),
            _ => Err(()),
        }
    }
}

fn main() {
    let network = env::var("NETWORK")
        .unwrap_or_else(|_| "mainnet".to_string())
        .parse::<Network>()
        .unwrap_or_else(|_| panic!("Invalid NETWORK value"));
    println!("Network: {:?}", network);

    let previous_tag = env::var("PREVIOUS_TAG").unwrap_or_else(|_| {
        match network {
            Network::Mainnet => "origin/testnet",
            Network::Testnet => "origin/devnet",
            Network::Devnet => "origin/devnet-ready",
        }
        .to_string()
    });

    let branch = env::var("BRANCH").unwrap_or(
        match network {
            Network::Mainnet => "origin/main",
            Network::Testnet => "origin/testnet",
            Network::Devnet => "origin/devnet",
        }
        .to_string(),
    );
    println!("Branch: {}", branch);

    println!(
        "Generating release notes for all merges since {}...",
        previous_tag,
    );
    let cmd = format!(
        "git log --merges --pretty=format:'%s' {}..{}",
        branch, previous_tag,
    );
    println!("$ {}", cmd);
    let merges = eval(cmd, false)
        .unwrap()
        .split("\n")
        .map(|s| s.trim().to_string())
        .filter(|s| {
            !s.is_empty()
                && s.starts_with("Merge pull request #")
                && !s.ends_with("from RaoFoundation/devnet-ready")
                && !s.ends_with("from RaoFoundation/testnet-ready")
                && !s.ends_with("from RaoFoundation/devnet")
                && !s.ends_with("from RaoFoundation/testnet")
                // Merges predating the opentensor -> RaoFoundation org move
                && !s.ends_with("from opentensor/devnet-ready")
                && !s.ends_with("from opentensor/testnet-ready")
                && !s.ends_with("from opentensor/devnet")
                && !s.ends_with("from opentensor/testnet")
        })
        .collect::<Vec<String>>();

    println!("");
    println!("Filtered merges:\n{}", merges.join("\n"));

    println!("");
    let pr_numbers = merges
        .iter()
        .map(|s| s.split(" ").collect::<Vec<&str>>()[3].trim_start_matches("#"))
        .collect::<Vec<&str>>();
    println!("PR numbers:\n{:?}", pr_numbers);

    println!("");
    println!("Fetching PR titles...");
    let pr_titles = pr_numbers
        .iter()
        .map(|pr_number| {
            print!("#{}: ", pr_number);
            let title = eval(format!("gh pr view {} --json title", pr_number), false)
                .unwrap()
                .trim()
                .to_string();
            if !title.starts_with("{\"title\":\"") {
                panic!("Malformed PR title: {}", title);
            }
            let title = title
                .trim_start_matches("{\"title\":\"")
                .trim_end_matches("\"}")
                .trim()
                .to_string();
            println!("{}", title);
            title
        })
        .collect::<Vec<String>>();

    println!("");
    println!("Fetching PR authors...");
    let pr_authors = pr_numbers
        .iter()
        .map(|pr_number| {
            print!("#{}: ", pr_number);
            let author = eval(
                format!("gh pr view {} --json author | jq .author.login", pr_number),
                false,
            )
            .unwrap()
            .trim()
            .trim_start_matches("\"")
            .trim_end_matches("\"")
            .to_string();
            println!("{}", author);
            author
        })
        .collect::<Vec<String>>();

    println!("");
    println!("generated release notes (gh comment):");
    let release_notes = "## What's Changed\n".to_string();
    let release_notes = release_notes
        + &pr_numbers
            .iter()
            .zip(pr_titles.iter())
            .zip(pr_authors.iter())
            .map(|((pr_number, pr_title), pr_author)| {
                format!("- #{} by @{}\n", pr_number, pr_author)
            })
            .collect::<String>();
    println!("{}", release_notes);

    println!("");
    println!("generated release notes (release):");
    let release_notes = "## What's Changed\n".to_string();
    let release_notes = release_notes
        + &pr_numbers
            .iter()
            .zip(pr_titles.iter())
            .zip(pr_authors.iter())
            .map(|((pr_number, pr_title), pr_author)| {
                format!("- {} in #{} by @{}\n", pr_title, pr_number, pr_author)
            })
            .collect::<String>();
    println!("{}", release_notes);

    println!("");
    println!("writing release notes to /tmp/release_notes.md");
    std::fs::write("/tmp/release_notes.md", release_notes).unwrap();
    println!("done!");
}
