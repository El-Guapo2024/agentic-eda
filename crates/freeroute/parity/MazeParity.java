// Dumps what FreeRouting v1.9 itself does when its batch autorouter routes
// a board's first connection, so the Rust port's maze search can be checked
// against it step by step.
//
// Loads a Specctra DSN board in 45-degree mode, with every search tree built
// under that mode, and prints the board as the router sees it -- layers,
// rules, router settings, every item with its raw shapes, and the shapes of
// both search trees -- then the connection BatchAutorouter would route
// first, and every step of FreeRouting's maze search for it. One record per
// line:
//
//   fixture <file>
//   board <llx> <lly> <urx> <ury>
//   layer <index> <is_signal 0|1> <name>
//   host_cad <0|1>
//   area_section <widest an area's tree shape may be>
//   min_trace_half_width <value>
//   default_via_diameter <value>
//   pin_edge_to_turn_dist <value>
//   classes <count>
//   cm <i> <j> <layer> <value>                    (nonzero; get_value(i, j) without margin)
//   cmax <i> <layer> <value>                      (ClearanceMatrix.max_value, nonzero)
//   padstack <no> <from layer> <to layer> <max width per layer, -1 for none>...
//   viainfo <index> <padstack no> <clearance class> <attach_smd 0|1>
//   viarule <index> <via info indices>...
//   netclass <index> <trace clearance class> <via rule index> <active 0|1 per layer> <half width per layer>...
//            <shove_fixed 0|1>
//   net <no> <net class index> <contains_plane 0|1>
//   settings <vias_allowed> <via_costs> <plane_via_costs> <start_ripup_costs> <with_fanout> <automatic_neckdown>
//   layer_costs <layer> <active 0|1> <horizontal> <vertical> <preferred direction costs>
//   it <id> <kind> <first layer> <last layer> <clearance class> <fixed state> <component> <nets>...
//   center <id> <x> <y>                           (pins and vias)
//   via_padstack <id> <padstack no>
//   pin_neckdown <id> <layer> <half width>        (Pin.get_trace_neckdown_halfwidth)
//   pin_exit <id> <layer> <dx> <dy>               (Pin.get_trace_exit_restrictions, in order)
//   conduction <id> <is_obstacle 0|1>
//   pad, trace, area, area_piece, outline, outline_shape  (as RoomParity writes them)
//   item, exact                                   (the autoroute tree, as RoomParity)
//   ditem <id> <shape index> <layer> <shape>      (the default tree, exact shapes)
//   route <net> <item id> <start ripup costs x pass>
//   start_item <id>... / dest_item <id>...
//   ctrl <name> <values>...                       (AutorouteControl, as built)
//   step <n> <door> <section> <sorting value> <expansion value> <backtrack door> <backtrack section>
//        <next room> <shape entry ax ay bx by> <room_ripped 0|1> <already_checked 0|1>
//   result <door> <section> | result none | maze none
//   path <door> <section>                         (backtrack from the result)
//   located <start item> <start layer> <target item> <target layer> | located none
//   located_trace <layer> <n> <x y>...            (LocateFoundConnectionAlgo's traces, in order)
//
// A door is "door <room> <room> <dimension>", "target <item id> <entry> <room>",
// "drill <x> <y> <first layer> <last layer>" or "page <llx> <lly> <urx> <ury>";
// a room is "r<id>" (free space), "o<item id>.<index>" (an obstacle's), or
// "none". Values are Java's shortest round-trip decimal.
//
// Licence: GPL-3.0, as it links FreeRouting.

import app.freerouting.autoroute.AutorouteControl;
import app.freerouting.autoroute.AutorouteEngine;
import app.freerouting.autoroute.CompleteFreeSpaceExpansionRoom;
import app.freerouting.autoroute.ExpandableObject;
import app.freerouting.autoroute.ExpansionDoor;
import app.freerouting.autoroute.ExpansionDrill;
import app.freerouting.autoroute.ExpansionRoom;
import app.freerouting.autoroute.LocateFoundConnectionAlgo;
import app.freerouting.autoroute.MazeSearchAlgo;
import app.freerouting.autoroute.MazeSearchElement;
import app.freerouting.autoroute.ObstacleExpansionRoom;
import app.freerouting.autoroute.TargetItemExpansionDoor;
import app.freerouting.board.AngleRestriction;
import app.freerouting.board.BoardObserverAdaptor;
import app.freerouting.board.BoardOutline;
import app.freerouting.board.ComponentObstacleArea;
import app.freerouting.board.ComponentOutline;
import app.freerouting.board.ConductionArea;
import app.freerouting.board.Connectable;
import app.freerouting.board.DrillItem;
import app.freerouting.board.Item;
import app.freerouting.board.ItemIdNoGenerator;
import app.freerouting.board.ObstacleArea;
import app.freerouting.board.Pin;
import app.freerouting.board.PolylineTrace;
import app.freerouting.board.RoutingBoard;
import app.freerouting.board.SearchTreeManager;
import app.freerouting.board.ShapeSearchTree;
import app.freerouting.board.TestLevel;
import app.freerouting.board.Via;
import app.freerouting.board.ViaObstacleArea;
import app.freerouting.datastructures.TimeLimit;
import app.freerouting.datastructures.UndoableObjects;
import app.freerouting.designforms.specctra.DsnFile;
import app.freerouting.geometry.planar.Area;
import app.freerouting.geometry.planar.Circle;
import app.freerouting.geometry.planar.ConvexShape;
import app.freerouting.geometry.planar.FloatLine;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntOctagon;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Line;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.PolygonShape;
import app.freerouting.geometry.planar.PolylineArea;
import app.freerouting.geometry.planar.PolylineShape;
import app.freerouting.geometry.planar.Shape;
import app.freerouting.geometry.planar.Simplex;
import app.freerouting.geometry.planar.TileShape;
import app.freerouting.interactive.AutorouteSettings;
import app.freerouting.interactive.BoardHandlingHeadless;
import app.freerouting.library.Padstack;
import app.freerouting.rules.ClearanceMatrix;
import app.freerouting.rules.Net;
import app.freerouting.rules.NetClass;
import app.freerouting.rules.ViaInfo;
import app.freerouting.rules.ViaRule;
import java.io.File;
import java.io.FileInputStream;
import java.io.InputStream;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.Iterator;
import java.util.List;
import java.util.Locale;
import java.util.Set;
import java.util.SortedSet;
import java.util.TreeSet;

public class MazeParity {

  public static void main(String[] args) throws Exception {
    if (args.length < 1) {
      System.err.println("usage: MazeParity <board.dsn> [max steps]");
      System.exit(2);
    }
    int max_steps = args.length > 1 ? Integer.parseInt(args[1]) : 2_000_000;
    BoardHandlingHeadless handling = new BoardHandlingHeadless(Locale.ENGLISH, false, 0f);
    try (InputStream in = new FileInputStream(args[0])) {
      DsnFile.ReadResult read =
          DsnFile.read(in, handling, new BoardObserverAdaptor(), new ItemIdNoGenerator(), TestLevel.RELEASE_VERSION);
      if (read != DsnFile.ReadResult.OK) {
        throw new IllegalStateException("could not read " + args[0] + ": " + read);
      }
    }
    RoutingBoard board = handling.get_routing_board();
    board.reduce_nets_of_route_items();
    // 45-degree mode throughout: every tree, the default one included, is
    // rebuilt under it, as if the board had been read in that mode.
    board.rules.set_trace_angle_restriction(AngleRestriction.FORTYFIVE_DEGREE);
    board.search_tree_manager.reset_compensated_trees();
    Method reinsert = SearchTreeManager.class.getDeclaredMethod("reinsert_tree_items");
    reinsert.setAccessible(true);
    reinsert.invoke(board.search_tree_manager);

    StringBuilder out = new StringBuilder();
    out.append("fixture ").append(new File(args[0]).getName()).append('\n');
    IntBox bb = board.get_bounding_box();
    out.append("board ").append(bb.ll.x).append(' ').append(bb.ll.y).append(' ').append(bb.ur.x).append(' ').append(bb.ur.y).append('\n');
    int layers = board.layer_structure.arr.length;
    for (int l = 0; l < layers; ++l) {
      out.append("layer ").append(l).append(' ').append(board.layer_structure.arr[l].is_signal ? 1 : 0).append(' ')
          .append(board.layer_structure.arr[l].name.replace(' ', '_')).append('\n');
    }
    out.append("host_cad ").append(board.communication.host_cad_exists() ? 1 : 0).append('\n');
    double area_section = 50000;
    if (board.communication.host_cad_exists()) {
      area_section = Math.min(500 * board.communication.get_resolution(app.freerouting.board.Unit.MIL), area_section);
    }
    out.append("area_section ").append(area_section).append('\n');
    out.append("min_trace_half_width ").append(board.get_min_trace_half_width()).append('\n');
    out.append("default_via_diameter ").append(board.rules.get_default_via_diameter()).append('\n');
    out.append("pin_edge_to_turn_dist ").append(board.rules.get_pin_edge_to_turn_dist()).append('\n');

    // Rules.
    ClearanceMatrix cm = board.rules.clearance_matrix;
    int classes = cm.get_class_count();
    out.append("classes ").append(classes).append('\n');
    for (int i = 0; i < classes; ++i) {
      for (int j = 0; j < classes; ++j) {
        for (int l = 0; l < layers; ++l) {
          int v = cm.get_value(i, j, l, false);
          if (v != 0) {
            out.append("cm ").append(i).append(' ').append(j).append(' ').append(l).append(' ').append(v).append('\n');
          }
        }
      }
      for (int l = 0; l < layers; ++l) {
        int v = cm.max_value(i, l);
        if (v != 0) {
          out.append("cmax ").append(i).append(' ').append(l).append(' ').append(v).append('\n');
        }
      }
    }
    for (int p = 1; p <= board.library.padstacks.count(); ++p) {
      Padstack ps = board.library.padstacks.get(p);
      out.append("padstack ").append(ps.no).append(' ').append(ps.from_layer()).append(' ').append(ps.to_layer());
      for (int l = 0; l < layers; ++l) {
        ConvexShape s = ps.get_shape(l);
        out.append(' ').append(s == null ? "-1" : Double.toString(s.max_width()));
      }
      out.append('\n');
    }
    List<ViaInfo> via_infos = new ArrayList<>();
    for (int i = 0; i < board.rules.via_infos.count(); ++i) {
      ViaInfo vi = board.rules.via_infos.get(i);
      via_infos.add(vi);
      out.append("viainfo ").append(i).append(' ').append(vi.get_padstack().no).append(' ').append(vi.get_clearance_class())
          .append(' ').append(vi.attach_smd_allowed() ? 1 : 0).append('\n');
    }
    for (int r = 0; r < board.rules.via_rules.size(); ++r) {
      ViaRule rule = board.rules.via_rules.get(r);
      out.append("viarule ").append(r);
      for (int i = 0; i < rule.via_count(); ++i) {
        out.append(' ').append(via_infos.indexOf(rule.get_via(i)));
      }
      out.append('\n');
    }
    List<NetClass> net_classes = new ArrayList<>();
    for (int c = 0; c < board.rules.net_classes.count(); ++c) {
      NetClass nc = board.rules.net_classes.get(c);
      net_classes.add(nc);
      out.append("netclass ").append(c).append(' ').append(nc.get_trace_clearance_class()).append(' ')
          .append(board.rules.via_rules.indexOf(nc.get_via_rule()));
      for (int l = 0; l < layers; ++l) {
        out.append(' ').append(nc.is_active_routing_layer(l) ? 1 : 0);
      }
      for (int l = 0; l < layers; ++l) {
        out.append(' ').append(nc.get_trace_half_width(l));
      }
      out.append(' ').append(nc.is_shove_fixed() ? 1 : 0).append('\n');
    }
    for (int n = 1; n <= board.rules.nets.max_net_no(); ++n) {
      Net net = board.rules.nets.get(n);
      if (net != null) {
        out.append("net ").append(n).append(' ').append(net_classes.indexOf(net.get_class())).append(' ')
            .append(net.contains_plane() ? 1 : 0).append('\n');
      }
    }
    AutorouteSettings settings = handling.get_settings().autoroute_settings;
    out.append("settings ").append(settings.get_vias_allowed() ? 1 : 0).append(' ').append(settings.get_via_costs()).append(' ')
        .append(settings.get_plane_via_costs()).append(' ').append(settings.get_start_ripup_costs()).append(' ')
        .append(settings.get_with_fanout() ? 1 : 0).append(' ').append(handling.get_settings().get_automatic_neckdown() ? 1 : 0)
        .append('\n');
    AutorouteControl.ExpansionCostFactor[] trace_costs = settings.get_trace_cost_arr();
    for (int l = 0; l < layers; ++l) {
      out.append("layer_costs ").append(l).append(' ').append(settings.get_layer_active(l) ? 1 : 0).append(' ')
          .append(trace_costs[l].horizontal).append(' ').append(trace_costs[l].vertical).append(' ')
          .append(settings.get_preferred_direction_trace_costs(l)).append('\n');
    }

    // Items, in the board's order, with their raw shapes.
    ShapeSearchTree default_tree = board.search_tree_manager.get_default_tree();
    Iterator<UndoableObjects.UndoableObjectNode> it = board.item_list.start_read_object();
    List<Item> items = new ArrayList<>();
    for (;;) {
      Item item = (Item) board.item_list.read_object(it);
      if (item == null) {
        break;
      }
      items.add(item);
    }
    for (Item item : items) {
      out.append("it ").append(item.get_id_no()).append(' ').append(kind(item)).append(' ').append(item.first_layer()).append(' ')
          .append(item.last_layer()).append(' ').append(item.clearance_class_no()).append(' ').append(item.get_fixed_state().ordinal())
          .append(' ').append(item.get_component_no());
      for (int i = 0; i < item.net_count(); ++i) {
        out.append(' ').append(item.get_net_no(i));
      }
      out.append('\n');
      if (item instanceof DrillItem drill) {
        IntPoint c = (IntPoint) drill.get_center();
        out.append("center ").append(item.get_id_no()).append(' ').append(c.x).append(' ').append(c.y).append('\n');
        for (int i = 0; i < drill.tile_shape_count(); ++i) {
          Shape shape = drill.get_shape(i);
          if (shape != null) {
            out.append("pad ").append(item.get_id_no()).append(' ').append(i).append(' ').append(drill.shape_layer(i)).append(' ')
                .append(item.clearance_class_no()).append(' ').append(pad_shape(shape)).append('\n');
          }
        }
      }
      if (item instanceof Via via) {
        out.append("via_padstack ").append(item.get_id_no()).append(' ').append(via.get_padstack().no).append('\n');
      }
      if (item instanceof Pin pin) {
        for (int l = pin.first_layer(); l <= pin.last_layer(); ++l) {
          out.append("pin_neckdown ").append(item.get_id_no()).append(' ').append(l).append(' ')
              .append(pin.get_trace_neckdown_halfwidth(l)).append('\n');
          for (Pin.TraceExitRestriction r : pin.get_trace_exit_restrictions(l)) {
            app.freerouting.geometry.planar.IntVector v = (app.freerouting.geometry.planar.IntVector) r.direction.get_vector();
            out.append("pin_exit ").append(item.get_id_no()).append(' ').append(l).append(' ').append(v.x).append(' ').append(v.y)
                .append('\n');
          }
        }
      }
      if (item instanceof PolylineTrace trace) {
        Line[] lines = trace.polyline().arr;
        out.append("trace ").append(item.get_id_no()).append(' ').append(trace.get_layer()).append(' ').append(trace.get_half_width())
            .append(' ').append(item.clearance_class_no()).append(' ').append(lines.length);
        for (Line l : lines) {
          out.append(' ').append(line(l));
        }
        out.append('\n');
      }
      if (item instanceof ObstacleArea area && area.get_area() != null) {
        out.append("area ").append(item.get_id_no()).append(' ').append(area.get_layer()).append(' ')
            .append(item.clearance_class_no()).append(' ').append(area_shape(area.get_area())).append('\n');
        if (item instanceof ConductionArea ca) {
          out.append("conduction ").append(item.get_id_no()).append(' ').append(ca.get_is_obstacle() ? 1 : 0).append('\n');
        }
      }
      if (item instanceof BoardOutline outline) {
        out.append("outline ").append(item.get_id_no()).append(' ').append(outline.get_half_width()).append(' ')
            .append(item.clearance_class_no()).append(' ').append(layers).append(' ')
            .append(outline.keepout_outside_outline_generated() ? 1 : 0).append('\n');
        for (int k = 0; k < outline.shape_count(); ++k) {
          PolylineShape shape = outline.get_shape(k);
          out.append("outline_shape ").append(shape.border_line_count());
          for (int i = 0; i < shape.border_line_count(); ++i) {
            out.append(' ').append(line(shape.border_line(i)));
          }
          out.append('\n');
        }
      }
    }

    // The connection BatchAutorouter.autoroute_pass routes first.
    Item first = null;
    Set<Item> handled_items = new TreeSet<>();
    for (Item item : items) {
      if (first != null) {
        break;
      }
      if (!(item instanceof Connectable) || item.is_routable() || handled_items.contains(item)) {
        continue;
      }
      for (int i = 0; i < item.net_count(); ++i) {
        int curr_net_no = item.get_net_no(i);
        Set<Item> connected_set = item.get_connected_set(curr_net_no);
        for (Item c : connected_set) {
          if (c.net_count() <= 1) {
            handled_items.add(c);
          }
        }
        if (connected_set.size() < board.connectable_item_count(curr_net_no) && !item.has_ignored_nets()) {
          first = item;
          break;
        }
      }
    }
    if (first == null) {
      out.append("route none\n");
      System.out.print(out);
      return;
    }
    int net_no = first.get_net_no(0);
    Net route_net = board.rules.nets.get(net_no);
    boolean contains_plane = route_net != null && route_net.contains_plane();
    int via_costs = contains_plane ? settings.get_plane_via_costs() : settings.get_via_costs();
    AutorouteControl ctrl = new AutorouteControl(board, net_no, handling.get_settings(), via_costs, trace_costs);
    ctrl.ripup_allowed = true;
    ctrl.ripup_costs = settings.get_start_ripup_costs();
    ctrl.remove_unconnected_vias = !settings.get_with_fanout();
    Set<Item> unconnected_set = first.get_unconnected_set(net_no);
    Set<Item> connected_set = first.get_connected_set(net_no);
    Set<Item> start_set = contains_plane ? connected_set : unconnected_set;
    Set<Item> dest_set = contains_plane ? unconnected_set : connected_set;

    // Both search trees, once the autoroute tree for the net's class exists.
    AutorouteEngine engine = board.init_autoroute(net_no, ctrl.trace_clearance_class_no, null, new TimeLimit(Integer.MAX_VALUE), false);
    ShapeSearchTree tree = engine.autoroute_search_tree;
    out.append("tree_class ").append(tree.compensated_clearance_class_no).append('\n');
    for (Item item : items) {
      for (int i = 0; i < item.tree_shape_count(tree); ++i) {
        TileShape shape = item.get_tree_shape(tree, i);
        if (shape == null) {
          continue;
        }
        IntOctagon bounds = shape.bounding_octagon();
        if (bounds == null) {
          continue;
        }
        out.append("item ").append(item.get_id_no()).append(' ').append(i).append(' ').append(item.shape_layer(i)).append(' ')
            .append(item.is_trace_obstacle(net_no) ? 1 : 0).append(' ').append(octagon(bounds))
            .append(shape instanceof IntOctagon || shape instanceof IntBox ? "" : " approx").append('\n');
        if (!(shape instanceof IntOctagon)) {
          out.append("exact ").append(item.get_id_no()).append(' ').append(i).append(' ').append(pad_shape(shape)).append('\n');
        }
      }
      for (int i = 0; i < item.tree_shape_count(default_tree); ++i) {
        TileShape shape = item.get_tree_shape(default_tree, i);
        out.append("ditem ").append(item.get_id_no()).append(' ').append(i).append(' ').append(item.shape_layer(i)).append(' ')
            .append(shape == null ? "none" : pad_shape(shape)).append('\n');
      }
    }

    out.append("route ").append(net_no).append(' ').append(first.get_id_no()).append(' ').append(ctrl.ripup_costs).append('\n');
    out.append("start_item");
    for (Item i : start_set) {
      out.append(' ').append(i.get_id_no());
    }
    out.append('\n').append("dest_item");
    for (Item i : dest_set) {
      out.append(' ').append(i.get_id_no());
    }
    out.append('\n');
    for (String name : new String[] {"trace_half_width", "compensated_trace_half_width", "via_radius_arr", "layer_active",
        "trace_clearance_class_no", "via_clearance_class", "min_normal_via_cost", "min_cheap_via_cost", "max_via_radius",
        "attach_smd_allowed", "vias_allowed", "with_neckdown", "via_lower_bound", "via_upper_bound",
        "max_shove_trace_recursion_depth", "max_shove_via_recursion_depth", "max_spring_over_recursion_depth"}) {
      out.append("ctrl ").append(name).append(' ').append(value(field(ctrl, name))).append('\n');
    }
    Object[] masks = (Object[]) field(ctrl, "via_info_arr");
    for (Object m : masks) {
      out.append("ctrl via_mask ").append(field(m, "from_layer")).append(' ').append(field(m, "to_layer")).append(' ')
          .append(((Boolean) field(m, "attach_smd_allowed")) ? 1 : 0).append('\n');
    }

    MazeSearchAlgo maze = MazeSearchAlgo.get_instance(start_set, dest_set, engine, ctrl);
    if (maze == null) {
      out.append("maze none\n");
      System.out.print(out);
      return;
    }
    @SuppressWarnings("unchecked")
    SortedSet<Object> list = (SortedSet<Object>) field(maze, "maze_expansion_list");
    for (int step = 0; step < max_steps; ++step) {
      for (Object e : list) {
        ExpandableObject door = (ExpandableObject) field(e, "door");
        int section = (Integer) field(e, "section_no_of_door");
        if (door.get_maze_search_element(section).is_occupied) {
          continue;
        }
        FloatLine entry = (FloatLine) field(e, "shape_entry");
        out.append("step ").append(step).append(' ').append(door(door)).append(' ').append(section).append(' ')
            .append(field(e, "sorting_value")).append(' ').append(field(e, "expansion_value")).append(' ')
            .append(door((ExpandableObject) field(e, "backtrack_door"))).append(' ').append(field(e, "section_no_of_backtrack_door"))
            .append(' ').append(room((ExpansionRoom) field(e, "next_room"))).append(' ')
            .append(entry == null ? "none" : entry.a.x + " " + entry.a.y + " " + entry.b.x + " " + entry.b.y).append(' ')
            .append((Boolean) field(e, "room_ripped") ? 1 : 0).append(' ').append((Boolean) field(e, "already_checked") ? 1 : 0)
            .append('\n');
        break;
      }
      if (!maze.occupy_next_element()) {
        break;
      }
    }
    if (System.getenv("MAZE_ROOMS") != null) {
      // Debugging aid: every completed room so far, with its doors.
      @SuppressWarnings("unchecked")
      List<CompleteFreeSpaceExpansionRoom> rooms = (List<CompleteFreeSpaceExpansionRoom>) field(engine, "complete_expansion_rooms");
      for (CompleteFreeSpaceExpansionRoom r : rooms) {
        out.append("xroom ").append(r.get_id_no()).append(' ').append(r.get_layer()).append(' ')
            .append(octagon(r.get_shape().bounding_octagon()));
        for (ExpansionDoor d : r.get_doors()) {
          out.append(" | ").append(door(d));
        }
        out.append('\n');
      }
    }
    ExpandableObject dest = (ExpandableObject) field(maze, "destination_door");
    if (dest != null) {
      // The found connection as traces: LocateFoundConnectionAlgo, which
      // also reallocates door sections, so it runs after the path is read.
      StringBuilder located_out = new StringBuilder();
      MazeSearchAlgo.Result found = maze.find_connection();
      LocateFoundConnectionAlgo located = LocateFoundConnectionAlgo.get_instance(
          found, ctrl, tree, board.rules.get_trace_angle_restriction(), new TreeSet<>(), new java.util.HashMap<>(), board.get_test_level());
      if (located == null || located.start_item == null) {
        located_out.append("located none\n");
      } else {
        located_out.append("located ").append(located.start_item.get_id_no()).append(' ').append(located.start_layer).append(' ')
            .append(located.target_item == null ? "none" : String.valueOf(located.target_item.get_id_no())).append(' ')
            .append(located.target_layer).append('\n');
        for (Object ri : located.connection_items) {
          IntPoint[] corners = (IntPoint[]) field(ri, "corners");
          located_out.append("located_trace ").append(field(ri, "layer")).append(' ').append(corners.length);
          for (IntPoint c : corners) {
            located_out.append(' ').append(c.x).append(' ').append(c.y);
          }
          located_out.append('\n');
        }
      }
      locate_records = located_out.toString();
    }
    if (dest == null) {
      out.append("result none\n");
    } else {
      int section = (Integer) field(maze, "section_no_of_destination_door");
      out.append("result ").append(door(dest)).append(' ').append(section).append('\n');
      ExpandableObject curr = dest;
      int curr_section = section;
      for (int guard = 0; curr != null && guard < 100_000; ++guard) {
        out.append("path ").append(door(curr)).append(' ').append(curr_section).append('\n');
        MazeSearchElement info = curr.get_maze_search_element(curr_section);
        curr = info.backtrack_door;
        curr_section = info.section_no_of_backtrack_door;
      }
    }
    out.append(locate_records);
    System.out.print(out);
  }

  /** The located connection's records, written after the path. */
  private static String locate_records = "";

  private static String kind(Item item) {
    if (item instanceof Pin) {
      return "pin";
    }
    if (item instanceof Via) {
      return "via";
    }
    if (item instanceof PolylineTrace) {
      return "trace";
    }
    if (item instanceof ConductionArea) {
      return "conduction";
    }
    if (item instanceof ViaObstacleArea) {
      return "via_keepout";
    }
    if (item instanceof ComponentObstacleArea) {
      return "component_keepout";
    }
    if (item instanceof ObstacleArea) {
      return "keepout";
    }
    if (item instanceof BoardOutline) {
      return "outline";
    }
    if (item instanceof ComponentOutline) {
      return "component_outline";
    }
    return "other_" + item.getClass().getSimpleName();
  }

  private static String door(ExpandableObject d) {
    if (d == null) {
      return "none";
    }
    if (d instanceof TargetItemExpansionDoor t) {
      return "target " + t.item.get_id_no() + " " + t.tree_entry_no + " " + room((ExpansionRoom) t.room);
    }
    if (d instanceof ExpansionDoor e) {
      return "door " + room(e.first_room) + " " + room(e.second_room) + " " + e.dimension;
    }
    if (d instanceof ExpansionDrill x) {
      IntPoint p = (IntPoint) x.location;
      return "drill " + p.x + " " + p.y + " " + x.first_layer + " " + x.last_layer;
    }
    if (d.getClass().getSimpleName().equals("DrillPage")) {
      IntBox b = (IntBox) d.get_shape();
      return "page " + b.ll.x + " " + b.ll.y + " " + b.ur.x + " " + b.ur.y;
    }
    return "other_" + d.getClass().getSimpleName();
  }

  private static String room(ExpansionRoom r) {
    if (r == null) {
      return "none";
    }
    if (r instanceof CompleteFreeSpaceExpansionRoom c) {
      return "r" + c.get_id_no();
    }
    if (r instanceof ObstacleExpansionRoom o) {
      return "o" + o.get_item().get_id_no() + "." + o.get_index_in_item();
    }
    return "inc";
  }

  private static Object field(Object o, String name) throws Exception {
    Class<?> c = o.getClass();
    while (c != null) {
      try {
        Field f = c.getDeclaredField(name);
        f.setAccessible(true);
        return f.get(o);
      } catch (NoSuchFieldException e) {
        c = c.getSuperclass();
      }
    }
    throw new NoSuchFieldException(name);
  }

  private static String value(Object v) {
    if (v instanceof int[] a) {
      StringBuilder b = new StringBuilder();
      for (int x : a) {
        b.append(b.length() == 0 ? "" : " ").append(x);
      }
      return b.toString();
    }
    if (v instanceof double[] a) {
      StringBuilder b = new StringBuilder();
      for (double x : a) {
        b.append(b.length() == 0 ? "" : " ").append(x);
      }
      return b.toString();
    }
    if (v instanceof boolean[] a) {
      StringBuilder b = new StringBuilder();
      for (boolean x : a) {
        b.append(b.length() == 0 ? "" : " ").append(x ? 1 : 0);
      }
      return b.toString();
    }
    if (v instanceof Boolean x) {
      return x ? "1" : "0";
    }
    return String.valueOf(v);
  }

  private static String line(Line l) {
    IntPoint a = (IntPoint) l.a;
    IntPoint e = (IntPoint) l.b;
    return a.x + " " + a.y + " " + e.x + " " + e.y;
  }

  private static String pad_shape(Shape shape) {
    if (shape instanceof Circle c) {
      return "circle " + c.center.x + " " + c.center.y + " " + c.radius;
    }
    if (shape instanceof IntBox b) {
      return "box " + b.ll.x + " " + b.ll.y + " " + b.ur.x + " " + b.ur.y;
    }
    if (shape instanceof IntOctagon o) {
      return "octagon " + octagon(o);
    }
    if (shape instanceof Simplex s) {
      StringBuilder b = new StringBuilder("simplex ").append(s.border_line_count());
      for (int i = 0; i < s.border_line_count(); ++i) {
        b.append(' ').append(line(s.border_line(i)));
      }
      return b.toString();
    }
    return "other " + shape.getClass().getSimpleName();
  }

  private static String area_shape(Area area) {
    if (area instanceof Circle c) {
      return "circle " + c.center.x + " " + c.center.y + " " + c.radius;
    }
    if (area instanceof PolylineArea pa) {
      StringBuilder b = new StringBuilder("holes ").append(pa.get_holes().length).append(' ').append(area_shape(pa.get_border()));
      for (PolylineShape hole : pa.get_holes()) {
        b.append(' ').append(area_shape(hole));
      }
      return b.toString();
    }
    if (area instanceof PolygonShape p) {
      StringBuilder b = new StringBuilder("polygon ").append(p.corners.length);
      for (Point corner : p.corners) {
        IntPoint q = (IntPoint) corner;
        b.append(' ').append(q.x).append(' ').append(q.y);
      }
      return b.toString();
    }
    if (area instanceof Shape s) {
      return pad_shape(s);
    }
    return "other " + area.getClass().getSimpleName();
  }

  private static String octagon(IntOctagon o) {
    return o.lx + " " + o.ly + " " + o.rx + " " + o.uy + " " + o.ulx + " " + o.lrx + " " + o.llx + " " + o.urx;
  }
}
