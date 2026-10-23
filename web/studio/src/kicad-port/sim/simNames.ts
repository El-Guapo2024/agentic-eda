// `NETLIST_EXPORTER_SPICE::ConvertToSpiceMarkup` (eeschema/netlist_exporters/netlist_exporter_spice.cpp, commit 8303b2ad): a net name as SPICE has it --
// an overbar `~{X}` becomes `~X`, sub- and superscript markup goes, the characters ngspice does not take become '_', and a root-sheet name starting "//"
// starts "/root/". The server has the same function (crates/sim/src/netlist.rs `spice_net_name`).

export function spiceNetName(name: string): string {
  let s = "";
  let depth = 0;
  for (let i = 0; i < name.length; i++) {
    const c = name.charAt(i);
    if ((c === "~" || c === "_" || c === "^") && name.charAt(i + 1) === "{") {
      if (c === "~") s += "~";
      depth++;
      i++;
      continue;
    }
    if (c === "}" && depth > 0) {
      depth--;
      continue;
    }
    s += c;
  }
  s = s.replace(/[%(),[\]<>~ ]/g, "_");
  if (s.startsWith("//")) s = s.replace("//", "/root/");
  return s;
}
