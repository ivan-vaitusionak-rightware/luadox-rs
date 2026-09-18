--- @class Link

--- Refers forward to @{Link.target}, to @{Link.target|a renamed target}, and to
--- `Link.target` in backticks.
---
--- Unresolvable: @{NoSuchThing}, @{NoSuchThing|with text}, and `NoSuchAlso`.
--- @see Link.target
--- @see NoSuchThing
--- @treturn Link The same object.
function Link.get()
end

--- The target field, declared after the function that references it.
Link.target = nil
