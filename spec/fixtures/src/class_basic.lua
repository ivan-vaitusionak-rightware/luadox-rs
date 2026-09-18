--- A widget on the screen.
---
--- The second paragraph is body text, not synopsis text.
--- @class Widget

--- The label shown on the widget.
--- @type string
Widget.label = nil

--- Moves the widget. The rest of this line is body, not synopsis.
---
--- A second paragraph of the method body.
--- @tparam number x Horizontal position.
--- @tparam number y Vertical position.
--- @treturn boolean Whether the move happened.
--- @treturn string A diagnostic message.
function Widget:move(x, y)
end

--- A function with no documented return at all.
function Widget.reset()
end
