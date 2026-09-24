--- Markdown shapes inside documentation. An indented code block is the recorded
-- CODE_INDENT deviation and is left out here.
-- @module markdown_edge
markdown_edge = {}

--- A fenced block that contains what looks like a tag.
--
-- ```lua
-- -- @tparam string x not a tag
-- local y = x
-- ```
--
-- @tparam string x the real one
function markdown_edge.fenced(x) end

--- Lists.
--
-- * one
-- * two
--   * nested
--
-- 1. first
-- 2. second
--
-- - dash
-- + plus
function markdown_edge.lists() end

--- Inline shapes: *em*, _em_, **strong**, `code`, ~~strike~~, <b>raw</b>, &amp; &lt; &, <, >, "quotes", 'single', unicode: ⛔ é ü 漢.
function markdown_edge.inline() end

--- Links: [text](http://x.y), <http://auto.link>, ![img](img/a.png), [ref][r], @{markdown_edge.lists|labelled}, `markdown_edge.lists`, @{nowhere}, `nowhere.at.all`, @{markdown_edge.lists}().
--
-- [r]: http://reference.link
function markdown_edge.links() end

--- Headings inside a doc.
--
-- # H1
-- ## H2
-- ### H3
--
-- Setext
-- ======
function markdown_edge.headings() end

--- A table with inline markdown and a pipe in code.
--
-- | a | b |
-- |---|---|
-- | `x \| y` | **z** |
-- | @{markdown_edge.lists} | |
function markdown_edge.table() end

--- Blockquote and rule.
--
-- > quoted
-- > lines
--
-- ---
--
-- after the rule
function markdown_edge.quote() end

--- Admonitions.
-- @note a note with `code`
-- @warning a warning
-- and its continuation
-- @see markdown_edge.lists
-- @see nowhere
-- @usage
-- local m = require("markdown_edge")
-- m.lists()
function markdown_edge.admonitions() end

--- Example from a file that does not exist.
-- @example missing_snippet.lua
function markdown_edge.example() end

--- Code tag.
-- @code
-- local a = 1
function markdown_edge.code() end

--- Two paragraphs with a hard break at the end of the first
-- continue here.
--
-- Second paragraph.
function markdown_edge.paragraphs() end
