# {name}

This is a Qxide project: `qxide.toml` is the package manifest, `src/main.qx` prepares a Bell pair and prints the two measured bits, and `.gitignore` ignores `target/`.

## Commands

```sh
qxide run              # run src/main.qx once
qxide run --shots 100  # run 100 shots
qxide check            # check types and qubit safety
qxide fmt src/main.qx  # format the source file
qxide qasm             # print the OpenQASM 3 circuit
qxide add <package>    # add and install a dependency
qxide repl             # start an interactive session
```

Run `qxide explain` followed by an error code such as `Q0302` to explain an error, or read the [language guide](https://www.qxide.dev/learn).
