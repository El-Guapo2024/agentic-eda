// A thin C ABI over KiCad's vendored Clipper2 2.0.1 (third_party/clipper2,
// copied from kicad-src-25ec2335/thirdparty/clipper2, built with USINGZ as
// KiCad's thirdparty/clipper2/CMakeLists.txt does). The Rust side
// (src/cpp.rs) keeps every input in Rust, so each call here builds one
// Clipper2 object, runs it and hands every output path to a sink callback:
// no C++ object outlives a call, and no C++ exception crosses the ABI.
//
// Point64 is passed as-is: with USINGZ it is { int64 x, y, z }, the same
// layout as the Rust #[repr(C)] Point64 (checked by the static_asserts).

#include "clipper2/clipper.h"

#include <cstddef>
#include <cstdint>

using namespace Clipper2Lib;

static_assert(sizeof(Point64) == 3 * sizeof(int64_t), "Point64 must be {x, y, z}");
static_assert(offsetof(Point64, x) == 0 && offsetof(Point64, y) == 8 && offsetof(Point64, z) == 16, "Point64 layout");

extern "C" {

struct c2_path {
    const Point64* pts;
    size_t len;
};

// One AddPaths call (engine: a = PathType, b = is_open) or one AddPaths of
// ClipperOffset (a = JoinType, b = EndType).
struct c2_group {
    const c2_path* paths;
    size_t count;
    int a;
    int b;
};

// kind: 0 = a closed path, 1 = an open path, 2 = a PolyTree node whose
// parent is the node emitted `parent` (-1 for a child of the root).
typedef void (*c2_sink)(void* ud, int kind, int64_t parent, const Point64* pts, size_t n);
typedef void (*c2_zcb)(void* ud, const Point64* e1bot, const Point64* e1top, const Point64* e2bot, const Point64* e2top, Point64* pt);

}

// Clipper64 with ClipperBase::AddPaths made public: the port's `add_paths`
// takes any PathType/is_open pair, which Clipper64's public AddSubject,
// AddOpenSubject and AddClip only cover in part.
class C2Clipper64 : public Clipper64
{
public:
    using ClipperBase::AddPaths;
};

static Paths64 to_paths(const c2_path* paths, size_t count)
{
    Paths64 out;
    out.reserve(count);
    for (size_t i = 0; i < count; ++i)
        out.emplace_back(paths[i].pts, paths[i].pts + paths[i].len);
    return out;
}

static void emit_paths(const Paths64& paths, int kind, c2_sink sink, void* ud)
{
    for (const Path64& p : paths)
        sink(ud, kind, -1, p.data(), p.size());
}

// Pre-order walk: a node is emitted before its children, children in order.
static void emit_tree(const PolyPath64& node, int64_t parent, int64_t& next, c2_sink sink, void* ud)
{
    for (const auto& child : node)
    {
        const int64_t me = next++;
        sink(ud, 2, parent, child->Polygon().data(), child->Polygon().size());
        emit_tree(*child, me, next, sink, ud);
    }
}

extern "C" {

// Clipper64: the AddPaths calls in order, then Execute (Paths64 or PolyTree64
// overload). Returns Execute's result; *error_code gets ErrorCode(), or -1
// when a C++ exception was caught.
int c2_clip(const c2_group* groups, size_t ngroups, int clip_type, int fill_rule, int preserve_collinear, int reverse_solution,
            int64_t default_z, c2_zcb zcb, void* zud, int as_tree, c2_sink sink, void* ud, int* error_code)
{
    try
    {
        C2Clipper64 c;
        c.PreserveCollinear(preserve_collinear != 0);
        c.ReverseSolution(reverse_solution != 0);
        c.DefaultZ = default_z;
        if (zcb)
            c.SetZCallback([zcb, zud](const Point64& e1bot, const Point64& e1top, const Point64& e2bot, const Point64& e2top, Point64& pt)
                           { zcb(zud, &e1bot, &e1top, &e2bot, &e2top, &pt); });
        for (size_t g = 0; g < ngroups; ++g)
            c.AddPaths(to_paths(groups[g].paths, groups[g].count), static_cast<PathType>(groups[g].a), groups[g].b != 0);

        bool ok;
        Paths64 open;
        if (as_tree)
        {
            PolyTree64 tree;
            ok = c.Execute(static_cast<ClipType>(clip_type), static_cast<FillRule>(fill_rule), tree, open);
            if (ok)
            {
                int64_t next = 0;
                emit_tree(tree, -1, next, sink, ud);
            }
        }
        else
        {
            Paths64 closed;
            ok = c.Execute(static_cast<ClipType>(clip_type), static_cast<FillRule>(fill_rule), closed, open);
            emit_paths(closed, 0, sink, ud);
        }
        emit_paths(open, 1, sink, ud);
        *error_code = c.ErrorCode();
        return ok ? 1 : 0;
    }
    catch (...)
    {
        *error_code = -1;
        return 0;
    }
}

// ClipperOffset: the AddPaths calls in order (a = JoinType, b = EndType),
// then Execute(delta, Paths64&) or Execute(delta, PolyTree64&).
int c2_offset(const c2_group* groups, size_t ngroups, double delta, double miter_limit, double arc_tolerance, int preserve_collinear,
              int reverse_solution, int as_tree, c2_sink sink, void* ud)
{
    try
    {
        ClipperOffset co(miter_limit, arc_tolerance, preserve_collinear != 0, reverse_solution != 0);
        for (size_t g = 0; g < ngroups; ++g)
            co.AddPaths(to_paths(groups[g].paths, groups[g].count), static_cast<JoinType>(groups[g].a), static_cast<EndType>(groups[g].b));
        if (as_tree)
        {
            PolyTree64 tree;
            co.Execute(delta, tree);
            int64_t next = 0;
            emit_tree(tree, -1, next, sink, ud);
        }
        else
        {
            Paths64 out;
            co.Execute(delta, out);
            emit_paths(out, 0, sink, ud);
        }
        return co.ErrorCode();
    }
    catch (...)
    {
        return -1;
    }
}

// RectClip64(rect).Execute(paths), or with free_fn the RectClip(rect, paths)
// wrapper of clipper.h. Returns 0, or -1 on a C++ exception.
int c2_rect_clip(int64_t left, int64_t top, int64_t right, int64_t bottom, const c2_path* paths, size_t count, int free_fn, c2_sink sink,
                 void* ud)
{
    try
    {
        const Rect64 rect(left, top, right, bottom);
        if (free_fn)
        {
            emit_paths(RectClip(rect, to_paths(paths, count)), 0, sink, ud);
            return 0;
        }
        RectClip64 rc(rect);
        emit_paths(rc.Execute(to_paths(paths, count)), 0, sink, ud);
        return 0;
    }
    catch (...)
    {
        return -1;
    }
}

// SimplifyPath (clipper.h). Returns 0, or -1 on a C++ exception.
int c2_simplify_path(const Point64* pts, size_t len, double epsilon, int is_closed_path, c2_sink sink, void* ud)
{
    try
    {
        Path64 path(pts, pts + len);
        Path64 out = SimplifyPath(path, epsilon, is_closed_path != 0);
        sink(ud, 0, -1, out.data(), out.size());
        return 0;
    }
    catch (...)
    {
        return -1;
    }
}

}
