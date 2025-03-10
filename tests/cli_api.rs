// // SPDX-FileCopyrightText: 2021-2024 Robin Vobruba <hoijui.quaero@gmail.com>
// //
// // SPDX-License-Identifier: AGPL-3.0-or-later

// use assert_cmd::Command; // Add methods on commands
// use predicates::prelude::*; // Used for writing assertions
// use std::{
//     collections::{hash_map::RandomState, HashMap},
//     fs,
//     path::Path,
// };

// pub struct Tester<'a, S = RandomState> {
//     cmd: &'a str,
//     cwd: Option<&'a str>,
//     stdin: Option<&'a str>,
//     env_vars: HashMap<&'a str, &'a str, S>,
//     args: Vec<&'a str>,
//     stdout: Option<&'a str>,
//     stderr: Option<&'a str>,
// }

// impl<'a> Tester<'a> {
//     pub fn new(cmd: &'a str) -> Tester<'a> {
//         Tester {
//             cmd,
//             cwd: None,
//             stdin: None,
//             env_vars: HashMap::new(),
//             args: Vec::new(),
//             stdout: None,
//             stderr: None,
//         }
//     }

//     /// Set the working directory for the child process.
//     pub fn cwd(&'a mut self, dir: &'a str) -> &'a mut Tester<'a> {
//         self.cwd = Some(dir);
//         self
//     }

//     pub fn stdin(&'a mut self, text: &'a str) -> &'a mut Tester<'a> {
//         self.stdin = Some(text);
//         self
//     }

//     /// Add an argument to pass to the program.
//     pub fn env(&'a mut self, key: &'a str, value: &'a str) -> &'a mut Tester<'a> {
//         self.env_vars.insert(key, value);
//         self
//     }

//     /// Add an argument to pass to the program.
//     pub fn arg(&'a mut self, arg: &'a str) -> &'a mut Tester<'a> {
//         self.args.push(arg);
//         self
//     }

//     /// Add multiple arguments to pass to the program.
//     pub fn args(&'a mut self, args: &[&'a str]) -> &'a mut Tester<'a> {
//         self.args.extend_from_slice(args);
//         self
//     }

//     pub fn stdout(&'a mut self, text: &'a str) -> &'a mut Tester<'a> {
//         self.stdout = Some(text);
//         self
//     }

//     pub fn stderr(&'a mut self, text: &'a str) -> &'a mut Tester<'a> {
//         self.stderr = Some(text);
//         self
//     }

//     pub fn run_test(&self) -> Result<(), Box<dyn std::error::Error>> {
//         let mut cmd = Command::cargo_bin(self.cmd)?;
//         cmd.env_clear();

//         // Prepares the command
//         for (key, value) in &self.env_vars {
//             cmd.env(key, value);
//         }
//         for arg in &self.args {
//             cmd.arg(arg);
//         }
//         if let Some(stdin) = self.stdin {
//             cmd.write_stdin(stdin);
//         }

//         // Runs the command
//         let mut assert = cmd.assert();

//         // Evaluates the command
//         assert = if let Some(stdout) = self.stdout {
//             assert.stdout(predicate::eq(stdout))
//         } else {
//             assert.failure()
//         };
//         if let Some(stderr) = self.stderr {
//             assert.stderr(predicate::str::contains(stderr));
//         }

//         Ok(())
//     }
// }

// pub fn write_to_file(file: &Path, text: &str) {
//     fs::write(file, text).expect("Unable to write file");
// }
