# {name}

This is a Qxide project where `main.qx` prepares a Bell pair and prints the two measured bits.

## Commands

```sh
qxide run main.qx              # run the program once
qxide run main.qx --shots 100  # run 100 shots
qxide check main.qx            # check types and qubit safety
qxide fmt main.qx              # format the source file
qxide qasm main.qx             # print the OpenQASM 3 circuit
qxide repl                     # start an interactive session
```

Run `qxide explain` followed by an error code such as `Q0302` to explain an error, or read the [language guide](https://www.qxide.dev/learn).
