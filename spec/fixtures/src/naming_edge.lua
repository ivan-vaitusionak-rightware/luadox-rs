--- Naming and scoping modifiers, all in one module, with a manual page that refers to them.
-- @module naming_edge
naming_edge = {}

--- A class inside the module.
-- @class naming_edge.Widget
-- @inherits naming_edge.Base, naming_edge.Mixin
naming_edge.Widget = {}

--- Base.
-- @class naming_edge.Base
naming_edge.Base = {}

--- Mixin.
-- @class naming_edge.Mixin
naming_edge.Mixin = {}

--- Renamed.
-- @rename Widget.shown
function naming_edge.Widget:show() end

--- Displayed differently.
-- @display widget.hide()
function naming_edge.Widget:hide() end

--- Scoped to the root.
-- @scope .
function naming_edge.Widget.helper() end

--- Scoped elsewhere.
-- @scope naming_edge.Base
function naming_edge.Widget.moved() end

--- Within a section on another page.
-- @within Utilities
function naming_edge.Widget.util() end

--- A section.
-- @section Utilities
-- @compact functions

--- In the section.
function naming_edge.sec_fn() end

--- Also in the section, a field.
-- @type number
naming_edge.sec_field = 1

--- An enum.
-- @enum naming_edge.Color
-- @compact fields
naming_edge.Color = {
    --- Red.
    RED = 1,
    --- Green.
    GREEN = 2,
    --- Not an integer.
    BLUE = "blue",
    UNDOCUMENTED = 4,
}

--- A nested table.
-- @table naming_edge.outer
naming_edge.outer = {
    --- Inner table.
    -- @table naming_edge.outer.inner
    inner = {
        --- Deep field.
        -- @type number
        deep = 1,
    },
    --- After the inner table closed.
    -- @type number
    shallow = 2,
}

--- Alias.
-- @alias ne
-- @alias naming_edge.alt
function naming_edge.aliased() end

--- References through the alias: @{ne.aliased}, @{naming_edge.alt.aliased}.
function naming_edge.refs() end

--- Ordering.
-- @order first
function naming_edge.z_first() end

--- Ordered after a missing anchor.
-- @order after nothing_here
function naming_edge.a_last() end

--- Fullnames.
-- @fullnames
-- @class naming_edge.Full
naming_edge.Full = {}

--- Member of Full.
-- @type number
naming_edge.Full.member = 1

--- Static under a class.
-- @type number
naming_edge.Widget.static.count = 0

--- A duplicate.
function naming_edge.refs() end
