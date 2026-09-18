--- @class Ordered

--- Members. This sentence is the section heading; this one is the body.
--- @section Members
--- @fullnames

--- Declared first, ordered last.
--- @order last
Ordered.beta = nil

--- Declared second, ordered first by default.
--- @meta read-only
Ordered.alpha = nil

--- Extras. A second section.
--- @section Extras

--- Declared in Extras, rendered in Members.
--- @within Members
--- @display renamed
Ordered.gamma = nil

--- Stays in Extras.
Ordered.delta = nil

--- Anchored ordering, and a @field injected from the comment block rather than from a
--- line of code.
--- @section Anchored

--- The anchor.
Ordered.middle = nil

--- Placed before the anchor even though it is declared after it.
--- @order before middle
Ordered.ahead = nil

--- Placed after the anchor.
--- @order after middle
Ordered.behind = nil

--- Anchored on a name nothing declares, which is reported rather than ignored.
--- @order before nosuchmember
Ordered.stray = nil

--- A block that declares a field itself.
--- @field injected A field with no line of code under it.

