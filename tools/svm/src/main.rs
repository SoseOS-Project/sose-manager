// svm: command line client for svmd, like qvm-start/qvm-shutdown in qubes.
//   svm list
//   svm start <name>
//   svm stop <name>
// it doesn't check the command itself, just forwards it and svmd decides

use std::env;

use svm_client::svm_request;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();

    // args[0] is the program itself, so skip it and glue the rest back into one line
    let cmd = args[1..].join(" ");
    // reply already ends with \n so print! not println!
    print!("{}", svm_request(&cmd)?);


    Ok(())
}
