--- A module with awkward comment shapes. Declarations inside `--[[ ]]` long comments
-- are a recorded deviation and are left out here.
-- @module comments_edge
comments_edge = {}

--- Four dashes below this line.
---- still the same block?
-- @type number
comments_edge.dashes = 1

---
-- @type number
comments_edge.empty_first_line = 2

--- Block one.
-- @type number
comments_edge.one = 3
--- Block two right after, no blank line.
-- @type number
comments_edge.two = 4

--- A block that documents nothing.
-- It ends at a blank line.

--- A block before `do`.
-- @type number
do
    comments_edge.in_do = 5
end

	--- Tab-indented block.
	-- @type number
	comments_edge.tabbed = 6

--- Doc with a --[[ long ]] comment marker inline in text.
-- @type number
comments_edge.inline_marker = 7

--- Doc ending with a fence that never closes.
-- ```
-- @type number
comments_edge.unclosed = 8
