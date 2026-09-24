--- A class whose tags are written every way people write them.
---@class Tight
Tight = {}

---@tparam string a no space after the dashes
--- @tparam number b a space
--@tparam boolean c only two dashes and no space
-- @treturn string first
-- @treturn number second
function Tight.spaced(a, b, c) end

--- Unknown tags are reported and ignored.
-- @foo bar
-- @Type string
function Tight.unknown() end

--- A cross reference at the start of a line is not a tag: @{Tight.spaced}.
-- @{Tight.unknown} starts this line.
-- @type string|number|nil
Tight.union = nil

--- Trailing spaces after a tag.
-- @type string
Tight.trailing = "x"

--- Deprecated twice.
-- @deprecated use Tight.spaced
-- @deprecated really
function Tight.old() end

--- A @meta on a function is resolved, on a field carried through.
-- @meta read-only see @{Tight.union}
function Tight.meta_fn() end

--- Field meta.
-- @meta read-only see @{Tight.union}
-- @type string
Tight.meta_field = "m"
