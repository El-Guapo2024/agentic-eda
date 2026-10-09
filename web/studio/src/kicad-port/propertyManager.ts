// `PROPERTY_MANAGER` (include/properties/property_mgr.h, common/properties/property_mgr.cpp at KiCad 8303b2ad), the class registry the Properties panel reads:
// which properties each item class has, in which order and group, which of a base class's it hides (`Mask`), replaces (`ReplaceProperty`), or makes
// available / writeable differently (`OverrideAvailability`, `OverrideWriteability`).
//
// What is ported is the part that decides the grid: the registration calls, the depth-first walk of `CLASS_DESC::collectPropsRecur` (a base class's
// properties get display indices BEFORE its derived class's, a class reached by two paths is walked twice and a mask only reaches what is walked after it),
// and the group order of `CLASS_DESC::rebuild`. What is not is C++ plumbing: getters and setters are functions of an item here, `wxAny` is `PropValue`,
// and a setter returns the commands that make the item have the value (the studio edits through undoable verbs, never by assigning to an item).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`). The registrations of an editor are in `pcbProperties.ts` and `schPropertyGrid.ts`; the grid
// built from them (`PROPERTIES_PANEL::rebuildProperties`) is `propertyGrid.ts`.

/** What a property holds. A value that is not one of the three is "unspecified": the items of a selection differ (`<...>`). */
export type PropValue = string | number | boolean;

/** One entry of an enum property (`wxPGChoices`): the text shown and the value stored. */
export interface Choice {
  label: string;
  value: string | number;
}

/** The widget family of the property (`TypeHash`): a string, an integer, a real number, a check box, an enumeration, a net, a colour. */
export type PropKind = "string" | "int" | "double" | "bool" | "enum" | "net" | "color";

/** `PROPERTY_DISPLAY`. An angle is always degrees here (`PT_DECIDEGREE` is a scale of the same thing). */
export type PropDisplay = "default" | "size" | "area" | "coord" | "degree" | "ratio" | "time";

/** An item of the selection, as far as the manager is concerned: its class (`TYPE_HASH( *item )`). */
export interface PropItem {
  readonly type: string;
}

/** `PROPERTY<Owner, T>`: one row of the grid for one class. `I` is the item, `C` what the getters read besides the item (the board, the units ...), `K` a command. */
export interface PropertyDef<I extends PropItem = PropItem, C = unknown, K = unknown> {
  /** The name shown, and the key a property is found by (`_HKI( "Position X" )`; compared without regard to case). */
  name: string;
  /** The class that registered it (`OwnerHash`). */
  owner: string;
  /** The class whose accessor it wraps (`BaseHash`), when that is not the owner (`PROPERTY<PCB_TEXT, bool, BOARD_ITEM>`): the key of `overrideAvailability`. */
  base?: string;
  kind: PropKind;
  display?: PropDisplay;
  /** The group the grid puts the row under; "" is "Basic Properties". Set by `addProperty`. */
  group?: string;
  /** The choices of an enum or net row, for this item (`SetChoicesFunc`, `PROPERTY_ENUM`'s `Choices()`). */
  choices?: (item: I, ctx: C) => readonly Choice[];
  /** `SetAvailableFunc`: does the item have this property at all. */
  available?: (item: I, ctx: C) => boolean;
  /** `SetWriteableFunc`: can it be edited right now. A property with no `set` is read-only whatever this says (`NO_SETTER`). */
  writeable?: (item: I, ctx: C) => boolean;
  /** `SetValidator`: a message when `value` is not allowed for the item, else null. */
  validate?: (value: PropValue, item: I, ctx: C) => string | null;
  /** `SetIsHiddenFromPropertiesManager`. */
  hidden?: boolean;
  /** `SetIsHiddenFromDesignEditors`. */
  hiddenFromDesignEditors?: boolean;
  /** The getter. */
  get: (item: I, ctx: C) => PropValue;
  /** The setter: the commands that make `item` have `value`; empty when it already has it. Absent: the row is read-only. */
  set?: (item: I, value: PropValue, ctx: C) => K[];
}

const key = (owner: string, name: string): string => `${owner}\u0000${name.toLowerCase()}`;

interface ClassDesc<I extends PropItem, C, K> {
  id: string;
  /** `m_bases`, in the order `InheritsAfter` was called. */
  bases: string[];
  /** `m_ownProperties` (by name) and `m_ownDisplayOrder`. */
  own: Map<string, PropertyDef<I, C, K>>;
  ownOrder: PropertyDef<I, C, K>[];
  masked: Set<string>;
  availabilityOverrides: Map<string, (item: I, ctx: C) => boolean>;
  writeabilityOverrides: Map<string, (item: I, ctx: C) => boolean>;
  replaced: Set<string>;
  /** The groups this class itself introduced, "" first (`m_groupDisplayOrder` before a rebuild). */
  ownGroups: string[];
  // built by `rebuild()`
  all: PropertyDef<I, C, K>[];
  displayOrder: Map<PropertyDef<I, C, K>, number>;
  groupOrder: string[];
}

export class PropertyManager<I extends PropItem = PropItem, C = unknown, K = unknown> {
  private classes = new Map<string, ClassDesc<I, C, K>>();
  private dirty = false;

  private getClass(id: string): ClassDesc<I, C, K> {
    let c = this.classes.get(id);
    if (!c) {
      c = {
        id,
        bases: [],
        own: new Map(),
        ownOrder: [],
        masked: new Set(),
        availabilityOverrides: new Map(),
        writeabilityOverrides: new Map(),
        replaced: new Set(),
        ownGroups: [""],
        all: [],
        displayOrder: new Map(),
        groupOrder: [""],
      };
      this.classes.set(id, c);
    }
    return c;
  }

  /** `REGISTER_TYPE`. */
  registerType(id: string): void {
    this.getClass(id);
  }

  /** `InheritsAfter( derived, base )`: bases are walked in the order they were declared. */
  inheritsAfter(derived: string, base: string): void {
    if (derived === base) throw new Error("a class cannot inherit from itself");
    this.getClass(base);
    this.getClass(derived).bases.push(base);
    this.dirty = true;
  }

  /**
   * `AddProperty( property, group )`: the property belongs to `def.owner`; the first one registered under a name keeps it (a second is dropped and the
   * first returned, as KiCad does). The group is created on the owner when new.
   */
  addProperty(def: PropertyDef<I, C, K>, group = ""): PropertyDef<I, C, K> {
    const cls = this.getClass(def.owner);
    const existing = cls.own.get(def.name);
    if (existing) return existing;
    def.group = group;
    cls.own.set(def.name, def);
    cls.ownOrder.push(def);
    if (!cls.ownGroups.includes(group)) cls.ownGroups.push(group);
    this.dirty = true;
    return def;
  }

  /** `ReplaceProperty( base, name, def, group )`: `def` stands in for the property `name` that `base` registered, on `def.owner` and the classes below it. */
  replaceProperty(base: string, name: string, def: PropertyDef<I, C, K>, group = ""): PropertyDef<I, C, K> {
    this.getClass(def.owner).replaced.add(key(base, name));
    return this.addProperty(def, group);
  }

  /** `Mask( derived, base, name )`: `derived` (and what is walked after it) does not show the property `name` that `base` registered. */
  mask(derived: string, base: string, name: string): void {
    this.getClass(derived).masked.add(key(base, name));
    this.dirty = true;
  }

  /** `OverrideAvailability`: for items of the class `derived` only (a subclass does not inherit it, which is why KiCad repeats them), `name` of `base` is available when `fn`. */
  overrideAvailability(derived: string, base: string, name: string, fn: (item: I, ctx: C) => boolean): void {
    this.getClass(derived).availabilityOverrides.set(key(base, name), fn);
  }

  /** `OverrideWriteability`: the same for writeability. */
  overrideWriteability(derived: string, base: string, name: string, fn: (item: I, ctx: C) => boolean): void {
    this.getClass(derived).writeabilityOverrides.set(key(base, name), fn);
  }

  /** `PROPERTY_MANAGER::Rebuild`. */
  rebuild(): void {
    for (const cls of this.classes.values()) this.rebuildClass(cls);
    this.dirty = false;
  }

  private ensureBuilt(): void {
    if (this.dirty) this.rebuild();
  }

  /** `CLASS_DESC::rebuild`. */
  private rebuildClass(cls: ClassDesc<I, C, K>): void {
    const replaced = new Set<string>();
    const masked = new Set<string>();
    const all: PropertyDef<I, C, K>[] = [];
    const order = new Map<PropertyDef<I, C, K>, number>();
    this.collectPropsRecur(cls, all, replaced, order, masked);
    cls.all = [...new Set(all)];
    cls.displayOrder = order;
    // The group order: the class's own groups, then each base's (and theirs), a group once.
    const groups: string[] = [];
    const seen = new Set<string>();
    const collect = (c: ClassDesc<I, C, K>): void => {
      for (const g of c.ownGroups) {
        if (!seen.has(g)) {
          seen.add(g);
          groups.push(g);
        }
      }
      for (const b of c.bases) collect(this.getClass(b));
    };
    collect(cls);
    cls.groupOrder = groups;
  }

  /**
   * `CLASS_DESC::collectPropsRecur`. A class puts its own properties at display indices below everything collected so far (so the base classes, which are
   * walked after it, end up above it), skips what a class walked before it replaced or masked, then walks its bases LAST DECLARED FIRST.
   */
  private collectPropsRecur(cls: ClassDesc<I, C, K>, result: PropertyDef<I, C, K>[], replaced: Set<string>, order: Map<PropertyDef<I, C, K>, number>, masked: Set<string>): void {
    for (const r of cls.replaced) replaced.add(r);
    for (const m of cls.masked) masked.add(m);
    let start = 0;
    if (order.size > 0) {
      let first = Infinity;
      for (const v of order.values()) first = Math.min(first, v);
      start = first - cls.own.size;
    }
    let idx = 0;
    for (const property of cls.ownOrder) {
      const k = key(property.owner, property.name);
      if (replaced.has(k)) continue;
      if (masked.has(k)) continue;
      order.set(property, start + idx++);
      result.push(property);
    }
    for (let i = cls.bases.length - 1; i >= 0; i--) this.collectPropsRecur(this.getClass(cls.bases[i]!), result, replaced, order, masked);
  }

  /** `GetProperties`: every property of the class, own and inherited, once each. */
  getProperties(type: string): readonly PropertyDef<I, C, K>[] {
    this.ensureBuilt();
    return this.classes.get(type)?.all ?? [];
  }

  /** `GetDisplayOrder`. */
  getDisplayOrder(type: string): ReadonlyMap<PropertyDef<I, C, K>, number> {
    this.ensureBuilt();
    return this.classes.get(type)?.displayOrder ?? new Map();
  }

  /** `GetGroupDisplayOrder`. */
  getGroupDisplayOrder(type: string): readonly string[] {
    this.ensureBuilt();
    return this.classes.get(type)?.groupOrder ?? [""];
  }

  /**
   * `GetProperty( type, name )`. KiCad picks the first of several properties of one name by their address; here, given the item and its context, the first of them that
   * the item has (a dimension's "Text" is two properties, one of them switched off for all but a leader), else the first.
   */
  getProperty(type: string, name: string, item?: I, ctx?: C): PropertyDef<I, C, K> | undefined {
    const lower = name.toLowerCase();
    const same = this.getProperties(type).filter((p) => p.name.toLowerCase() === lower);
    if (same.length <= 1 || item === undefined) return same[0];
    return same.find((p) => this.isAvailableFor(type, p, item, ctx as C)) ?? same[0];
  }

  /** `IsAvailableFor`: the property's own test, then the item's class's override of it (only the class itself: overrides are not inherited). */
  isAvailableFor(type: string, property: PropertyDef<I, C, K>, item: I, ctx: C): boolean {
    if (property.available && !property.available(item, ctx)) return false;
    const override = this.classes.get(type)?.availabilityOverrides.get(key(property.base ?? property.owner, property.name));
    return override ? override(item, ctx) : true;
  }

  /** `IsWriteableFor`: `PROPERTY::Writeable` is false without a setter. */
  isWriteableFor(type: string, property: PropertyDef<I, C, K>, item: I, ctx: C): boolean {
    if (!property.set) return false;
    if (property.writeable && !property.writeable(item, ctx)) return false;
    const override = this.classes.get(type)?.writeabilityOverrides.get(key(property.base ?? property.owner, property.name));
    return override ? override(item, ctx) : true;
  }

  /** `IsOfType( derived, base )`. */
  isOfType(derived: string, base: string): boolean {
    if (derived === base) return true;
    const cls = this.classes.get(derived);
    return !!cls && cls.bases.some((b) => this.isOfType(b, base));
  }
}
