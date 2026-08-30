# Den wiring — the dendritic spine of this flake.
#
# import-tree (flake.nix) loads every .nix file under nix/modules/ as a
# flake-parts module; den's flakeModule adds the aspect system on top so
# FTS features can be published as aspects and consumed from the system
# flake (and vice versa) without copy-paste.
#
# Today the heavy outputs (shells, crane packages, images) are ordinary
# perSystem modules speaking the `fts.*` schema (options.nix). New
# shareable features (e.g. an `fts-rig` aspect carrying the systemd user
# unit + udev rules for a rig host) belong here as den.aspects.<name>.
{ inputs, ... }:
{
  imports = [ inputs.den.flakeModule ];
}
