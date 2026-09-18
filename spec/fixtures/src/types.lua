--- @class Color

--- @class Shape

--- Paints a shape.
--- @tparam bool on Whether to paint.
--- @tparam int count How many times.
--- @tparam float ratio The ratio.
--- @tparam double precise The precise ratio.
--- @tparam Color color A documented class used as a type.
--- @tparam Unknown thing A name that resolves to nothing.
--- @tparam engine::Color scoped A C++ scoped name.
--- @tparam string|nil label A union of two built-ins.
--- @treturn void
function Shape.paint(on, count, ratio, precise, color, thing, scoped, label)
end

--- A function whose parameter has no @tparam at all.
function Shape.undocumented(a)
end
