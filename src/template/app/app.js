// The page calls the public .qx functions listed in [app] commands through `qxide.invoke`.

const T = {
  intro: "Click Measure to prepare a Bell pair in src/app.qx and measure both qubits for the chosen number of shots, producing results that always agree (00 or 11).",
  shots: "Shots",
  measure: "Measure",
  save: "Save results",
  running: "Measuring…",
  done: "Measured {shots} shots",
  saved: "Saved to results.txt in the app data folder:",
};

const $ = (id) => document.getElementById(id);
let last = null;

function show(counts, shots) {
  const chart = $("chart");
  chart.replaceChildren();
  const by = new Map(counts.map((r) => [r.value.join(""), r.count]));
  for (const key of ["00", "01", "10", "11"]) {
    const n = by.get(key) ?? 0;
    const row = document.createElement("div");
    row.className = "row";
    const label = document.createElement("span");
    label.className = "key";
    label.textContent = key;
    const bar = document.createElement("span");
    bar.className = "bar";
    bar.style.width = `${(100 * n) / shots}%`;
    const value = document.createElement("span");
    value.className = "count";
    value.textContent = String(n);
    row.append(label, bar, value);
    chart.append(row);
  }
}

async function measure() {
  const shots = Math.max(1, Math.min(1_000_000, Number($("shots").value) || 1000));
  $("status").textContent = T.running;
  $("measure").disabled = true;
  try {
    const r = await qxide.invoke("measure_bell", {}, { shots });
    const counts = r.counts ?? [{ value: r.value, count: 1 }];
    show(counts, shots);
    last = counts.map((c) => `${c.value.join("")}=${c.count}`).join(" ");
    $("save").disabled = false;
    $("status").textContent = `${T.done.replace("{shots}", shots)} (seed ${r.seed})`;
  } catch (e) {
    $("status").textContent = e.rendered ?? String(e);
  } finally {
    $("measure").disabled = false;
  }
}

async function save() {
  try {
    const line = await qxide.invoke("save_result", { text: last });
    $("status").textContent = `${T.saved} ${line.value}`;
  } catch (e) {
    $("status").textContent = e.rendered ?? String(e);
  }
}

$("intro").textContent = T.intro;
$("shots-label").textContent = T.shots;
$("measure").textContent = T.measure;
$("save").textContent = T.save;
$("measure").addEventListener("click", measure);
$("save").addEventListener("click", save);
