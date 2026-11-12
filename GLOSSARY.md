# Flipclock

A realistic 3D split-flap desktop clock that shows the current time as six flipping digit tiles plus a date line.

## Clock face

**Tile**:
One digit position on the clock face, with its own thickness, bevel and hinge. There are six: hours, minutes and seconds, two each.
_Avoid_: Card, leaf, panel, digit box

**Flap**:
One of the two hinged halves of a tile. The top flap shows the upper half of the current digit, and its back carries the lower half of the next digit.
_Avoid_: Leaf, card, half

**Separator**:
The pair of lit 3D dots between two digit pairs.
_Avoid_: Colon, divider

**Date line**:
The single line of text that shows the weekday and date in the selected date language.
_Avoid_: Date label, subtitle

**Clock area**:
The part of the window the clock face fills: the whole window, minus the settings panel while it is open.

**Battery indicator**:
The battery icon with its charge percentage, shown in a corner of the clock area on computers that have a battery.
_Avoid_: Battery widget, power icon

## Countdown

**Countdown**:
A timer set by the user, shown on the tiles in place of the time and counting down to zero. It can be idle (being set), running, paused or finished.
_Avoid_: Timer, stopwatch

**Description**:
The optional text the user gives a countdown, shown above the tiles.
_Avoid_: Label, title, name

**Alarm**:
The beeping played when a countdown reaches zero.
_Avoid_: Ring, bell

## Motion

**Flip**:
One rotation of a tile's top flap, which changes the digit the tile shows. The clock advances one digit per flip; a countdown flips straight to the new digit.

**Roll**:
A sequence of consecutive flips that a tile makes to reach a target digit more than one step away, passing through every intermediate digit.
_Avoid_: Spin, fast-forward

**Cascade**:
The staggered start of the flips across several tiles that change in the same second, from right to left.

**Tick**:
The sound played when a flap lands.
_Avoid_: Click
