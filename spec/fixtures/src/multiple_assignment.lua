--- A chord, analysed from notes.
-- @class Chord
Chord = {}

--- Sorts notes and finds the root.
-- @tparam tab notes the notes to analyse
-- @treturn tab the notes in chord order
-- @treturn string the root note
function Chord.analyze(notes)
    return notes, "C"
end

function Chord:update(notes)
    --- The root note, assigned alongside a local in one statement.
    -- @type string
    notes, self.root = Chord.analyze(notes)
end

--- Two locals in one statement; the one before the `=` is documented.
-- @type int
local first, second = 1, 2
