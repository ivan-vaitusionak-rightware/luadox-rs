--- @class Shell
--- @display The Shell

--- Statics. Members here are requalified onto Shell.
--- @section Statics
--- @scope Shell

--- A field whose source symbol is requalified by the section's @scope.
Internal.thing = nil

--- @class Renamed
--- @rename Actual

--- A class reachable under a second name.
--- @class Aliased
--- @alias Alias

--- Returns an aliased thing.
--- @treturn Alias The aliased class, referenced by its alias.
function Shell.make()
end
