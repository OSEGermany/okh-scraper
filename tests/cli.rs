// // SPDX-FileCopyrightText: 2021 Robin Vobruba <hoijui.quaero@gmail.com>
// //
// // SPDX-License-Identifier: AGPL-3.0-or-later

// mod cli_api;

// use cli_api::write_to_file;
// use cli_api::Tester;
// // Add methods on commands
// use tempfile::NamedTempFile;

// const CMD: &str = "repvar";

// #[test]
// fn simple() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .arg("-DKEY=value")
//         .stdin("This text contains a ${KEY} in its middle.")
//         .stdout("This text contains a value in its middle.")
//         .run_test()
// }

// #[test]
// fn simple_with_dollar_val() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .arg("-DKEY=value$1")
//         .stdin("This text contains a ${KEY} in its middle.")
//         .stdout("This text contains a value$1 in its middle.")
//         .run_test()
// }

// #[test]
// fn simple_space() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .arg("-D")
//         .arg("KEY=value")
//         .stdin("This text contains a ${KEY} in its middle.")
//         .stdout("This text contains a value in its middle.")
//         .run_test()
// }

// #[test]
// fn simple_equals() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .arg("-D=KEY=value")
//         .stdin("This text contains a ${KEY} in its middle.")
//         .stdout("This text contains a value in its middle.")
//         .run_test()
// }

// #[test]
// fn simple_long() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .arg("--variable")
//         .arg("KEY=value")
//         .stdin("This text contains a ${KEY} in its middle.")
//         .stdout("This text contains a value in its middle.")
//         .run_test()
// }

// #[test]
// fn simple_long_equals() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .arg("--variable=KEY=value")
//         .stdin("This text contains a ${KEY} in its middle.")
//         .stdout("This text contains a value in its middle.")
//         .run_test()
// }

// #[test]
// fn simple_new_line() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .arg("-DKEY=value")
//         .stdin("This text contains a ${KEY} in its middle.\n")
//         .stdout("This text contains a value in its middle.\n")
//         .run_test()
// }

// #[test]
// fn quoting() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .arg("-DKEY=value")
//         .stdin("This text contains a quoted $${KEY}.")
//         .stdout("This text contains a quoted ${KEY}.")
//         .run_test()
// }

// #[test]
// fn file_does_not_exist() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .arg("--input")
//         .arg("test/file/does/not/exist")
//         .stderr("No such file or directory")
//         .run_test()
// }

// #[test]
// fn file_does_exist() -> Result<(), Box<dyn std::error::Error>> {
//     let file = NamedTempFile::new()?;
//     write_to_file(file.path(), "This text contains a ${KEY} in its middle.\n");
//     let file_path_string = file.path().as_os_str().to_str().ok_or("Non UTF-8 string")?;

//     Tester::new(CMD)
//         .arg("-DKEY=value")
//         .arg("-i")
//         .arg(file_path_string)
//         .stdout("This text contains a value in its middle.\n")
//         .run_test()
// }

// #[test]
// fn var_file_does_not_exist() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .arg("--variables-file")
//         .arg("test/file/does/not/exist")
//         .stderr("No such file or directory")
//         .run_test()
// }

// #[test]
// fn var_file_does_not_exist_short() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .arg("-I")
//         .arg("test/file/does/not/exist")
//         .stderr("No such file or directory")
//         .run_test()
// }

// #[test]
// fn var_file_does_exist() -> Result<(), Box<dyn std::error::Error>> {
//     let file = NamedTempFile::new()?;
//     write_to_file(file.path(), "KEY=value\n");
//     let file_path_string = file.path().as_os_str().to_str().ok_or("Non UTF-8 string")?;

//     Tester::new(CMD)
//         .arg("--variables-file")
//         .arg(file_path_string)
//         .stdin("This text contains a ${KEY} in its middle.\n")
//         .stdout("This text contains a value in its middle.\n")
//         .run_test()
// }

// #[test]
// fn var_file_no_newline() -> Result<(), Box<dyn std::error::Error>> {
//     let file = NamedTempFile::new()?;
//     write_to_file(file.path(), "KEY=value");
//     let file_path_string = file.path().as_os_str().to_str().ok_or("Non UTF-8 string")?;

//     Tester::new(CMD)
//         .arg("--variables-file")
//         .arg(file_path_string)
//         .stdin("This text contains a ${KEY} in its middle.\n")
//         .stdout("This text contains a value in its middle.\n")
//         .run_test()
// }

// #[test]
// fn var_file_multi_props() -> Result<(), Box<dyn std::error::Error>> {
//     let file = NamedTempFile::new()?;
//     write_to_file(file.path(), "KEY_1=value1\nKEY_2=value2\nKEY_3=value3\n");
//     let file_path_string = file.path().as_os_str().to_str().ok_or("Non UTF-8 string")?;

//     Tester::new(CMD)
//         .arg("--variables-file")
//         .arg(file_path_string)
//         .stdin("This text contains three values - ${KEY_1}, ${KEY_2}, ${KEY_3} - in its middle.\n")
//         .stdout("This text contains three values - value1, value2, value3 - in its middle.\n")
//         .run_test()
// }

// #[test]
// fn var_file_with_dollar_unescaped() -> Result<(), Box<dyn std::error::Error>> {
//     let file = NamedTempFile::new()?;
//     write_to_file(file.path(), "KEY=value$1\n");
//     let file_path_string = file.path().as_os_str().to_str().ok_or("Non UTF-8 string")?;

//     Tester::new(CMD)
//         .arg("--variables-file")
//         .arg(file_path_string)
//         .stdin("This text contains a ${KEY} in its middle.\n")
//         .stdout("This text contains a value in its middle.\n")
//         .run_test()
// }

// #[test]
// fn var_file_with_dollar_escaped() -> Result<(), Box<dyn std::error::Error>> {
//     let file = NamedTempFile::new()?;
//     write_to_file(file.path(), "KEY=value\\$1\n");
//     let file_path_string = file.path().as_os_str().to_str().ok_or("Non UTF-8 string")?;

//     Tester::new(CMD)
//         .arg("--variables-file")
//         .arg(file_path_string)
//         .stdin("This text contains a ${KEY} in its middle.\n")
//         .stdout("This text contains a value$1 in its middle.\n")
//         .run_test()
// }

// #[test]
// fn file_does_exist_long() -> Result<(), Box<dyn std::error::Error>> {
//     let file = NamedTempFile::new()?;
//     write_to_file(file.path(), "This text contains a ${KEY} in its middle.\n");
//     let file_path_string = file.path().as_os_str().to_str().ok_or("Non UTF-8 string")?;

//     Tester::new(CMD)
//         .arg("-DKEY=value")
//         .arg("--input")
//         .arg(file_path_string)
//         .stdout("This text contains a value in its middle.\n")
//         .run_test()
// }

// #[test]
// fn fail_not_on_missing_values() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .stdin("This text contains a ${KEY} in its middle.")
//         .stdout("This text contains a ${KEY} in its middle.")
//         .run_test()
// }

// #[test]
// fn fail_on_missing_values() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .arg("-f")
//         .stdin("This text contains a ${KEY} in its middle.")
//         .stderr("Undefined variable 'KEY'")
//         .run_test()
// }

// #[test]
// fn fail_on_missing_values_long() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .arg("--fail-on-missing-values")
//         .stdin("This text contains a ${KEY} in its middle.")
//         .stderr("Undefined variable 'KEY'")
//         .run_test()
// }

// #[test]
// fn env() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .env("KEY", "value")
//         .arg("-e")
//         .stdin("This text contains a ${KEY} in its middle.")
//         .stdout("This text contains a value in its middle.")
//         .run_test()
// }

// #[test]
// fn env_long() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .env("KEY", "value")
//         .arg("--env")
//         .stdin("This text contains a ${KEY} in its middle.")
//         .stdout("This text contains a value in its middle.")
//         .run_test()
// }

// #[test]
// fn env_disabled() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .env("KEY", "value")
//         .stdin("This text contains a ${KEY} in its middle.")
//         .stdout("This text contains a ${KEY} in its middle.")
//         .run_test()
// }

// #[test]
// fn env_disabled_fail() -> Result<(), Box<dyn std::error::Error>> {
//     Tester::new(CMD)
//         .env("KEY", "value")
//         .arg("--fail-on-missing-values")
//         .stdin("This text contains a ${KEY} in its middle.")
//         .stderr("Undefined variable 'KEY'")
//         .run_test()
// }
