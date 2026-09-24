-- The module is named like the file: the Python never registers a module declared in
-- the same block as a section, and dies on any name that is not also the file's.
---
-- @module module_then_section
-- @section Paths

--- Base directory for generated files.
-- @type string
base_path = "/tmp/settings"

--- How many entries to keep.
-- @type int
max_entries = 500

--- Reloads the settings from disk.
function reload()
end
