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
