// Writes LICENSE.md for the packaged extension from the repository's LICENSE-MIT and
// LICENSE-APACHE, so the .vsix carries the dual license without a second copy in git.
// vsce only recognises LICENSE, LICENSE.md, or LICENSE.txt and otherwise prompts for input.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "..", "..");
const read = (name) => readFileSync(join(root, name), "utf8").replace(/\r\n/g, "\n").trimEnd();

// Same wording as the License section of the repository README.
const parts = [
  "Licensed under either of MIT (LICENSE-MIT) or Apache-2.0 (LICENSE-APACHE) at your option.",
  "## LICENSE-MIT",
  "```text\n" + read("LICENSE-MIT") + "\n```",
  "## LICENSE-APACHE",
  "```text\n" + read("LICENSE-APACHE") + "\n```",
];
writeFileSync(join(here, "..", "LICENSE.md"), parts.join("\n\n") + "\n");
