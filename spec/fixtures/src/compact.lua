--- @class Config

--- Flags. Everything after the first sentence stays in the section body.
--- @section Flags
--- @compact fields

--- Enables logging.
Config.verbose = false

--- Enables tracing.
--- @deprecated Use verbose instead.
Config.trace = false

--- Calls. This section compacts both fields and functions.
--- @section Calls
--- @compact

--- The retry count.
Config.retries = 3

--- Reloads the configuration.
--- @tparam boolean force Reload even when unchanged.
function Config.reload(force)
end
