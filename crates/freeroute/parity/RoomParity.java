// Dumps what FreeRouting v1.9 itself does when it fills a board's free space
// with rooms, so the Rust port can be checked against it room for room.
//
// Loads a Specctra DSN board, switches it to 45-degree routing, and for one
// net completes rooms from a start point until none is left incomplete --
// the same fill the Rust parity test runs. Prints, one record per line:
//
//   fixture <file>
//   board <llx> <lly> <urx> <ury>
//   net <net_no>
//   item <id> <shape_index> <layer> <obstacle 0|1> <octagon> [approx]
//                                                              (tree insertion order)
//   tree_class <clearance class the tree is built for>
//   cm <class i> <class j> <layer> <clearance>                 (nonzero entries)
//   pad <item id> <shape_index> <layer> <clearance class> <shape>
//                                                              (pins and vias)
//   area_section <widest an area's tree shape may be>
//   area <item id> <layer> <clearance class> <shape>           (keepouts, pours)
//   start <layer> <x> <y>
//   room <id> <net_dependent 0|1> <layer> <octagon>            (completion order)
//   door <room id> <other room id> <dimension>                 (each room's doors, in order)
//   obstacle_doors <count>
//
// Doors to obstacle expansion rooms -- rooms FreeRouting makes around
// routable items for ripup and push, not ported yet -- are only counted.
// They never change the free-space rooms.
//
// With --steps, each completion is recorded as well, before the rooms:
//
//   step <n> <layer> <shape|none> <contained> <id of the room ignored, or 0>
//   grown <count>, then per candidate: cand <shape> <contained>
//   made <ids of the rooms completed>
//
// "grown" is what complete_shape returns for that room, so a divergence
// can be placed before or after it.
//
// An item whose tree shape is not an octagon -- a trace segment, until
// traces are ported -- is given by its bounding octagon and marked approx;
// the room check skips such boards, as FreeRouting tests those shapes
// exactly. A pad shape is "circle cx cy r", "box llx lly urx ury",
// "octagon <octagon>", or "simplex <n> <ax ay bx by>..." for a convex
// polygon's n border lines; an area is "circle cx cy r", or "other <kind>"
// where the port cannot take it yet. An octagon is lx ly rx uy ulx lrx llx urx, the
// Java field order. Run by dump.sh against FreeRouting's v1.9 jar.
//
// Licence: GPL-3.0, as it links FreeRouting.

import app.freerouting.autoroute.AutorouteEngine;
import app.freerouting.autoroute.CompleteFreeSpaceExpansionRoom;
import app.freerouting.autoroute.ExpansionDoor;
import app.freerouting.autoroute.ExpansionRoom;
import app.freerouting.autoroute.IncompleteFreeSpaceExpansionRoom;
import app.freerouting.board.AngleRestriction;
import app.freerouting.board.BoardObserverAdaptor;
import app.freerouting.board.DrillItem;
import app.freerouting.board.Item;
import app.freerouting.board.ItemIdNoGenerator;
import app.freerouting.board.ObstacleArea;
import app.freerouting.board.Pin;
import app.freerouting.board.RoutingBoard;
import app.freerouting.board.SearchTreeObject;
import app.freerouting.board.ShapeSearchTree;
import app.freerouting.board.TestLevel;
import app.freerouting.datastructures.UndoableObjects;
import app.freerouting.designforms.specctra.DsnFile;
import app.freerouting.geometry.planar.Circle;
import app.freerouting.geometry.planar.IntBox;
import app.freerouting.geometry.planar.IntOctagon;
import app.freerouting.geometry.planar.IntPoint;
import app.freerouting.geometry.planar.Line;
import app.freerouting.geometry.planar.Point;
import app.freerouting.geometry.planar.Shape;
import app.freerouting.geometry.planar.Simplex;
import app.freerouting.geometry.planar.TileShape;
import app.freerouting.interactive.BoardHandlingHeadless;
import app.freerouting.rules.Net;
import java.io.File;
import java.io.FileInputStream;
import java.io.InputStream;
import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Comparator;
import java.util.Iterator;
import java.util.List;
import java.util.Locale;

public class RoomParity {

  public static void main(String[] args) throws Exception {
    List<String> positional = new ArrayList<>();
    boolean steps = false;
    for (String a : args) {
      if (a.equals("--steps")) {
        steps = true;
      } else {
        positional.add(a);
      }
    }
    if (positional.isEmpty()) {
      System.err.println("usage: RoomParity <board.dsn> [net_no] [--steps]");
      System.exit(2);
    }
    args = positional.toArray(new String[0]);
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
    // The port covers 45-degree routing. The tree type is fixed when a tree
    // is first built, so drop any built before the switch.
    board.rules.set_trace_angle_restriction(AngleRestriction.FORTYFIVE_DEGREE);
    board.search_tree_manager.reset_compensated_trees();

    int net_no = args.length > 1 ? Integer.parseInt(args[1]) : first_routable_net(board);
    Net net = board.rules.nets.get(net_no);
    AutorouteEngine engine = new AutorouteEngine(board, net.get_class().get_trace_clearance_class(), false);
    engine.init_connection(net_no, null, null);
    ShapeSearchTree tree = engine.autoroute_search_tree;

    StringBuilder out = new StringBuilder();
    out.append("fixture ").append(new File(args[0]).getName()).append('\n');
    IntBox bb = board.get_bounding_box();
    out.append("board ").append(bb.ll.x).append(' ').append(bb.ll.y).append(' ').append(bb.ur.x).append(' ').append(bb.ur.y).append('\n');
    out.append("net ").append(net_no).append('\n');

    // SearchTreeManager.get_autoroute_tree inserts the items in this order,
    // each item's shapes by index.
    Iterator<UndoableObjects.UndoableObjectNode> it = board.item_list.start_read_object();
    for (;;) {
      Item item = (Item) board.item_list.read_object(it);
      if (item == null) {
        break;
      }
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
            .append(shape instanceof IntOctagon ? "" : " approx").append('\n');
      }
    }

    // The clearance rules the tree shapes were grown by.
    out.append("tree_class ").append(tree.compensated_clearance_class_no).append('\n');
    int classes = board.rules.clearance_matrix.get_class_count();
    int layers = board.layer_structure.arr.length;
    for (int i = 0; i < classes; ++i) {
      for (int j = 0; j < classes; ++j) {
        for (int l = 0; l < layers; ++l) {
          int v = board.rules.clearance_matrix.get_value(i, j, l, false);
          if (v != 0) {
            out.append("cm ").append(i).append(' ').append(j).append(' ').append(l).append(' ').append(v).append('\n');
          }
        }
      }
    }
    // The raw shapes of pins and vias, from which their tree shapes grow.
    it = board.item_list.start_read_object();
    for (;;) {
      Item item = (Item) board.item_list.read_object(it);
      if (item == null) {
        break;
      }
      if (!(item instanceof DrillItem drill)) {
        continue;
      }
      for (int i = 0; i < drill.tile_shape_count(); ++i) {
        Shape shape = drill.get_shape(i);
        if (shape == null) {
          continue;
        }
        out.append("pad ").append(item.get_id_no()).append(' ').append(i).append(' ').append(drill.shape_layer(i)).append(' ')
            .append(item.clearance_class_no()).append(' ').append(pad_shape(shape)).append('\n');
      }
    }
    // Areas: their raw shapes, and how wide their tree shapes may be
    // (ShapeSearchTree.calculate_tree_shapes(ObstacleArea)).
    double area_section = 50000;
    if (board.communication.host_cad_exists()) {
      area_section = Math.min(500 * board.communication.get_resolution(app.freerouting.board.Unit.MIL), area_section);
    }
    out.append("area_section ").append(area_section).append('\n');
    it = board.item_list.start_read_object();
    for (;;) {
      Item item = (Item) board.item_list.read_object(it);
      if (item == null) {
        break;
      }
      if (item instanceof ObstacleArea area && area.get_area() != null) {
        String shape = area.get_area() instanceof Circle c
            ? "circle " + c.center.x + " " + c.center.y + " " + c.radius
            : "other " + area.get_area().getClass().getSimpleName();
        out.append("area ").append(item.get_id_no()).append(' ').append(area.get_layer()).append(' ')
            .append(item.clearance_class_no()).append(' ').append(shape).append('\n');
      }
    }

    // Start at the centre of the net's first pin, on its first layer.
    List<Pin> pins = new ArrayList<>(net.get_pins());
    pins.sort(Comparator.comparingInt(Item::get_id_no));
    Pin pin = pins.get(0);
    Point centre = pin.get_center();
    IntPoint c = centre instanceof IntPoint ? (IntPoint) centre : centre.to_float().round();
    out.append("start ").append(pin.first_layer()).append(' ').append(c.x).append(' ').append(c.y).append('\n');

    engine.add_incomplete_expansion_room(null, pin.first_layer(), c.surrounding_octagon());
    int step = 0;
    for (IncompleteFreeSpaceExpansionRoom room; (room = engine.get_first_incomplete_expansion_room()) != null; ++step) {
      if (steps) {
        // What complete_expansion_room ignores: the completed room behind
        // the first overlap door.
        TileShape door_shape = null;
        SearchTreeObject ignore = null;
        int ignore_id = 0;
        for (ExpansionDoor door : room.get_doors()) {
          ExpansionRoom other = door.other_room((ExpansionRoom) room);
          if (other instanceof CompleteFreeSpaceExpansionRoom && door.dimension == 2) {
            door_shape = door.get_shape();
            ignore = (CompleteFreeSpaceExpansionRoom) other;
            ignore_id = other.get_id_no();
            break;
          }
        }
        out.append("step ").append(step).append(' ').append(room.get_layer()).append(' ')
            .append(room.get_shape() == null ? "none" : octagon((IntOctagon) room.get_shape())).append(' ')
            .append(octagon(room.get_contained_shape().bounding_octagon())).append(' ').append(ignore_id).append('\n');
        Collection<IncompleteFreeSpaceExpansionRoom> grown = tree.complete_shape(room, net_no, ignore, door_shape);
        out.append("grown ").append(grown.size()).append('\n');
        for (IncompleteFreeSpaceExpansionRoom g : grown) {
          out.append("cand ").append(octagon((IntOctagon) g.get_shape())).append(' ').append(octagon(g.get_contained_shape().bounding_octagon())).append('\n');
        }
      }
      Collection<CompleteFreeSpaceExpansionRoom> made = engine.complete_expansion_room(room);
      if (steps) {
        out.append("made");
        for (CompleteFreeSpaceExpansionRoom m : made) {
          out.append(' ').append(m.get_id_no());
        }
        out.append('\n');
      }
      if (step > 200_000) {
        throw new IllegalStateException("fill did not finish");
      }
    }

    List<CompleteFreeSpaceExpansionRoom> rooms = complete_rooms(engine);
    for (CompleteFreeSpaceExpansionRoom room : rooms) {
      out.append("room ").append(room.get_id_no()).append(' ').append(room.is_net_dependent() ? 1 : 0).append(' ')
          .append(room.get_layer()).append(' ').append(octagon((IntOctagon) room.get_shape())).append('\n');
    }
    int obstacle_doors = 0;
    for (CompleteFreeSpaceExpansionRoom room : rooms) {
      for (ExpansionDoor door : room.get_doors()) {
        ExpansionRoom other = door.other_room((ExpansionRoom) room);
        if (!(other instanceof CompleteFreeSpaceExpansionRoom)) {
          ++obstacle_doors;
          continue;
        }
        out.append("door ").append(room.get_id_no()).append(' ').append(other.get_id_no()).append(' ').append(door.dimension).append('\n');
      }
    }
    out.append("obstacle_doors ").append(obstacle_doors).append('\n');
    System.out.print(out);
  }

  /** The lowest-numbered net with at least two pins. */
  private static int first_routable_net(RoutingBoard board) {
    for (int n = 1; n <= board.rules.nets.max_net_no(); ++n) {
      Net net = board.rules.nets.get(n);
      if (net != null && net.get_pins().size() >= 2) {
        return n;
      }
    }
    throw new IllegalStateException("no net with two pins");
  }

  @SuppressWarnings("unchecked")
  private static List<CompleteFreeSpaceExpansionRoom> complete_rooms(AutorouteEngine engine) throws Exception {
    Field f = AutorouteEngine.class.getDeclaredField("complete_expansion_rooms");
    f.setAccessible(true);
    List<CompleteFreeSpaceExpansionRoom> rooms = (List<CompleteFreeSpaceExpansionRoom>) f.get(engine);
    return rooms == null ? List.of() : rooms;
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
        Line l = s.border_line(i);
        IntPoint a = (IntPoint) l.a;
        IntPoint e = (IntPoint) l.b;
        b.append(' ').append(a.x).append(' ').append(a.y).append(' ').append(e.x).append(' ').append(e.y);
      }
      return b.toString();
    }
    return "other " + shape.getClass().getSimpleName();
  }

  private static String octagon(IntOctagon o) {
    return o.lx + " " + o.ly + " " + o.rx + " " + o.uy + " " + o.ulx + " " + o.lrx + " " + o.llx + " " + o.urx;
  }
}
