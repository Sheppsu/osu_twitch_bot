osu twitch bot with np, pp, and ppnow commands. Initially made it for mrekk's chat but here's the code so feel free to use it. Only runs on windows atm.

check [releases](https://github.com/Sheppsu/osu_twitch_bot/releases) if you just want a binary

initially running the binary will create a setup.cfg and ask you to fill in the values

use of commands:
- (5 second cd) !np - shows current map
- (sub only, 3 second cd) !pp [acc] [+mods] - order doesn't matter, acc doesn't require ending with a %, but mods must start with a +. With no args will default to current mods and 100% acc. Can specify +NM for no mods. If used on the results screen it will say the pp for that acc and mods, however, you can still specify different mods or acc.
- (mods only, 1 second cd) !ppnow - shows current pp count during gameplay
