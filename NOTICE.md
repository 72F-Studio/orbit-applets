# Notice

This is a modified version of [cosmic-applets](https://github.com/pop-os/cosmic-applets)
by System76, kept for the Orbit scrolling tiling compositor (`orbit-comp`). It
is not a System76 project and is not supported by the COSMIC developers.

## Licence

GPL-3.0-only, the same as upstream. The upstream copyright notices are kept
unchanged.

## Modifications (GPL-3.0 section 5(a))

Only `cosmic-applet-tiling/` is changed. It adds a **Scrolling** mode beside
Floating and Tiling, which writes the compositor's `tiling_engine` setting, and
a matching panel icon.

| Date       | Change                                                                 |
|------------|------------------------------------------------------------------------|
| 2026-09-14 | Applet changes written by miguel-das on top of upstream `e01b16f4`.    |
| 2026-11-15 | Imported into this fork and rebased onto upstream `epoch-1.11.0`.      |

## Origin and credit

The changes come from
[miguel-das/cosmic_scrolling_prototype](https://github.com/miguel-das/cosmic_scrolling_prototype).
That project describes itself as an unmaintained proof of concept and invites
forks. According to its README, it was AI-generated (GPT 5.6 Sol) and tested by
its author. The import commit keeps miguel-das as its author.
