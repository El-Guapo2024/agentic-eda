//! The little of kicad-cli's generic XML netlist (`kicad-cli sch export netlist --format kicadxml`, `NETLIST_EXPORTER_XML`) the simulator
//! needs: every component's fields as the derived schematic has them (the `Sim.*` fields among them), its pins, and the nets with the pins on
//! them. It says, from kicad-cli, what `SIM_LIB_MGR::CreateModel` reads off a symbol (`SCH_EDITOR_CONTROL::SimProbe` and the tuners ask it),
//! without a second reading of the design here.

use std::collections::BTreeMap;

/// One `<comp>`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Comp {
    pub reference: String,
    pub value: String,
    /// `<fields><field name=..>` -- the symbol's own fields but Reference and Value.
    pub fields: BTreeMap<String, String>,
    /// The symbol's pin numbers, in the order of its `<units>`.
    pub pins: Vec<String>,
    /// `<libsource lib part>`.
    pub lib_id: String,
}

/// One `<net>`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XmlNet {
    pub name: String,
    /// `(reference, pin number)` of every `<node>`.
    pub nodes: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct XmlNetlist {
    pub comps: Vec<Comp>,
    pub nets: Vec<XmlNet>,
}

impl XmlNetlist {
    pub fn comp(&self, reference: &str) -> Option<&Comp> {
        self.comps.iter().find(|c| c.reference == reference)
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let ent = &tail[1..end];
        match ent {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ if ent.starts_with("#x") => out.extend(u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32)),
            _ if ent.starts_with('#') => out.extend(ent[1..].parse::<u32>().ok().and_then(char::from_u32)),
            _ => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

fn attrs(tag: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let mut rest = tag;
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq].split_whitespace().last().unwrap_or("").to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
        let body = &after[1..];
        let Some(close) = body.find(q) else { break };
        m.insert(name, unescape(&body[..close]));
        rest = &body[close + 1..];
    }
    m
}

/// Read the netlist. Elements other than those above are skipped; a file that is not one gives an empty netlist.
pub fn parse_xml_netlist(text: &str) -> XmlNetlist {
    let mut out = XmlNetlist::default();
    let mut stack: Vec<String> = Vec::new();
    let mut comp: Option<Comp> = None;
    let mut net: Option<XmlNet> = None;
    let mut field_name: Option<String> = None;
    let mut text_buf = String::new();
    let mut rest = text;
    while let Some(lt) = rest.find('<') {
        text_buf.push_str(&rest[..lt]);
        let after = &rest[lt + 1..];
        let Some(gt) = after.find('>') else { break };
        let tag = &after[..gt];
        rest = &after[gt + 1..];
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        if let Some(name) = tag.strip_prefix('/') {
            let name = name.trim();
            let content = unescape(text_buf.trim());
            text_buf.clear();
            match name {
                "comp" => out.comps.extend(comp.take()),
                "net" => out.nets.extend(net.take()),
                "value" if stack.iter().rev().nth(1).is_some_and(|p| p == "comp") => {
                    if let Some(c) = comp.as_mut() {
                        c.value = content;
                    }
                }
                "field" => {
                    if let (Some(c), Some(f)) = (comp.as_mut(), field_name.take()) {
                        c.fields.insert(f, content);
                    }
                }
                _ => {}
            }
            stack.pop();
            continue;
        }
        let self_closing = tag.ends_with('/');
        let body = tag.trim_end_matches('/');
        let name = body.split_whitespace().next().unwrap_or("").to_string();
        let a = attrs(&body[name.len()..]);
        text_buf.clear();
        let in_components = stack.iter().any(|s| s == "components");
        match name.as_str() {
            "comp" if in_components => comp = Some(Comp { reference: a.get("ref").cloned().unwrap_or_default(), ..Comp::default() }),
            "field" if comp.is_some() => {
                let n = a.get("name").cloned().unwrap_or_default();
                if self_closing {
                    if let Some(c) = comp.as_mut() {
                        c.fields.insert(n, String::new());
                    }
                } else {
                    field_name = Some(n);
                }
            }
            "libsource" if comp.is_some() => {
                if let Some(c) = comp.as_mut() {
                    c.lib_id = format!("{}:{}", a.get("lib").map_or("", String::as_str), a.get("part").map_or("", String::as_str));
                }
            }
            "pin" if comp.is_some() => {
                if let (Some(c), Some(num)) = (comp.as_mut(), a.get("num")) {
                    c.pins.push(num.clone());
                }
            }
            "net" => net = Some(XmlNet { name: a.get("name").cloned().unwrap_or_default(), nodes: Vec::new() }),
            "node" => {
                if let Some(n) = net.as_mut() {
                    n.nodes.push((a.get("ref").cloned().unwrap_or_default(), a.get("pin").cloned().unwrap_or_default()));
                }
            }
            _ => {}
        }
        if !self_closing {
            stack.push(name);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_fields_pins_and_nets_are_read() {
        let x = r#"<?xml version="1.0" encoding="UTF-8"?>
<export version="E">
  <components>
    <comp ref="U1">
      <value>OPAMP</value>
      <fields>
        <field name="Sim.Pins">1=in+ 2=in- 3=vcc 4=vee 5=out</field>
        <field name="Footprint"/>
        <field name="Note">a &amp; b</field>
      </fields>
      <libsource lib="Simulation_SPICE" part="OPAMP" description=""/>
      <units><unit name="A"><pins><pin num="1"/><pin num="2"/></pins></unit></units>
    </comp>
  </components>
  <libparts><libpart lib="X" part="Y"><fields><field name="Reference">U</field></fields><pins><pin num="9" name="" type="passive"/></pins></libpart></libparts>
  <nets>
    <net code="1" name="/IN" class="Default">
      <node ref="R1" pin="1" pintype="passive"/>
      <node ref="U1" pin="2" pintype="input"/>
    </net>
  </nets>
</export>"#;
        let n = parse_xml_netlist(x);
        assert_eq!(n.comps.len(), 1);
        let u = n.comp("U1").unwrap();
        assert_eq!(u.value, "OPAMP");
        assert_eq!(u.fields["Sim.Pins"], "1=in+ 2=in- 3=vcc 4=vee 5=out");
        assert_eq!(u.fields["Note"], "a & b");
        assert_eq!(u.fields["Footprint"], "");
        assert_eq!(u.pins, vec!["1", "2"], "the libpart's pins are not the comp's");
        assert_eq!(u.lib_id, "Simulation_SPICE:OPAMP");
        assert_eq!(n.nets[0].name, "/IN");
        assert_eq!(n.nets[0].nodes[1], ("U1".to_string(), "2".to_string()));
    }
}
