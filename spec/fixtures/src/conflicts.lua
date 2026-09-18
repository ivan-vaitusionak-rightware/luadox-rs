--- @class Twin

--- The first field.
Twin.duplicate = 1

--- The second field under the same name, which does not replace the first.
Twin.duplicate = 2

--- A section whose name repeats on another page. Sections are not qualified by their
--- page, so this one is allowed to repeat and must not be reported.
--- @section Shared

--- Lives in the shared-named section.
Twin.inShared = 3

--- @class Other

--- The same section name again, on a second page.
--- @section Shared

--- Ordering. @order first, which nothing else exercises.
--- @section Ordering

--- Declared first, pushed last.
--- @order last
Other.last = nil

--- Declared second, no order: ends up between the two.
Other.middle = nil

--- Declared third, pulled to the front.
--- @order first
Other.first = nil
