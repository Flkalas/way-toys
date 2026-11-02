# Third-party notices

## Microsoft PowerToys FancyZones

`crates/waytoys-zones` and the interaction controller in
`crates/waytoys-zones-editor` contain Rust adaptations of Microsoft PowerToys
FancyZones data structures, defaults, geometry calculations and editor algorithms.

- Upstream: https://github.com/microsoft/PowerToys
- Reference commit: `8827f848fbbac4afeedcae2907c71cd05fd5aa86`
- Copyright (c) Microsoft Corporation. All rights reserved.
- License: MIT, preserved verbatim in [LICENSE-MIT-PowerToys](LICENSE-MIT-PowerToys).

Source files used include `FancyZonesLib/Settings.h`, `Layout.cpp`,
`LayoutConfigurator.cpp`, `HighlightedZones.cpp`, `DraggingState.cpp`,
`FancyZonesEditorCommon/Data/CustomLayouts.cs`, `LayoutDefaultSettings.cs`,
`editor/FancyZonesEditor/Models/CanvasLayoutModel.cs`, `GridLayoutModel.cs`,
`GridData.cs`, `CanvasZone.xaml.cs`, `MagneticSnap.cs`, and editor backup helpers.
Adapted files identify their source in the file header. The Windows UI and
window-management implementation is not incorporated into this crate.

## KWin native adapter

`kwin/zones` links against KWin and is distributed under GPL-2.0-or-later,
as declared in its source and plugin metadata. A copy of GPL version 2 is
included in [kwin/zones/LICENSE](kwin/zones/LICENSE).
The Rust programs communicate with this adapter over the session bus.
