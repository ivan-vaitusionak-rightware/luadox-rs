--- @class Snip

--- Shows how the three code forms render.
---
--- A fenced block written by hand:
---
--- ```lua
--- local s = Snip.demo()
--- ```
---
--- @example
---   local s = Snip.demo()
---   print(s)
---
--- Text after the inline example.
--- @example lua hello.lua
--- @example lua nosuchfile.lua
function Snip.demo()
end

--- The two code forms the production corpus never uses.
---
--- @usage heads its block the way @example does; @code heads nothing.
--- @usage
---   Snip.other()
--- @code
---   -- no heading above this one
--- @code text
---   plain text, not lua
function Snip.other()
end
