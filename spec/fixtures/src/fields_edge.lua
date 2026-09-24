--- Every assignment shape the two implementations agree on. The shapes they disagree
-- on by design -- a value spanning lines, `--` inside a string, a comparison, a tab
-- before the `=` -- are in harness/improvements.toml, not here.
-- @module fields_edge
fields_edge = {}

--- Bracket string key.
-- @type number
fields_edge["key"] = 1

--- Bracket single-quoted key.
-- @type number
fields_edge['single'] = 2

--- Bracket integer key.
-- @type number
fields_edge[1] = 3

--- Two targets, two values.
-- @type number
fields_edge.a, fields_edge.b = 1, 2

--- Function value has no literal.
fields_edge.f = function(x) return x end

--- Assigned from another field.
-- @type function
fields_edge.g = fields_edge.f

--- Varargs.
function fields_edge.h(...) end

--- Colon method with an unknown tag variant.
-- @tparam number a first
-- @tparam[opt] number b second
function fields_edge:i(a, b) end

--- A local function.
local function j() end

--- A nested function in a table constructor.
fields_edge.k = { l = function() end }

--- Semicolon separators.
fields_edge.m = { 1; 2; 3 }

--- A field named like a keyword.
-- @type number
fields_edge["end"] = 4
