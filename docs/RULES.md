# Catan Rules Summary

Source: [CATAN 6th Edition, CN3081, v6.250401](https://www.catan.com/sites/default/files/2025-03/CN3081%20CATAN%E2%80%93The%20Game%20Rulebook%20secure%20%281%29.pdf).
The downloaded PDF is located in [`res/catan-rulebook.pdf`](res/catan-rulebook.pdf).
This is an implementation-oriented summary for `rust-catan`. If this file and the PDF disagree, the PDF wins.

## Objective

- First player to **10 victory points (VPs) on their own turn** wins immediately.
- You may reveal any number of Victory Point dev cards — even ones bought this turn — to reach 10.

## Components (3–4 players)

- 19 terrain hexes: 4 forest, 4 pasture, 4 fields, 3 hills, 3 mountains, 1 desert.
- 18 number discs (2–12, no 7).
- 95 resource cards (19× each): wood, brick, wool, wheat, ore.
- 25 development cards: 14 Knight, 5 Victory Point, 2 Monopoly, 2 Road Building, 2 Invention (Year of Plenty).
- Per color: 15 roads, 5 settlements, 4 cities.
- 1 robber, Longest Route tile, Largest Army tile, ports (3:1 and 2:1), 2 dice, sea frame.

Terrain → resource:
- Forest → wood, Hills → brick, Pasture → wool, Fields → wheat, Mountains → ore, Desert → nothing.

## Setup

Fixed setup (recommended first game, p.4–5):
1. Assemble sea frame.
2. Place hexes + number discs as diagrammed.
3. Create supply: 5 face-up resource stacks, dev cards face-down. Place bonus tiles beside board.
4. Robber starts on desert. Each player picks a color, takes pieces + player aid. 3-player game: white unused.
5. Place 2 settlements + 2 roads per player as shown, then collect starting resources from hexes adjacent to the 2nd settlement.
6. Highest dice roll goes first.

Variable setup (p.11, post-MVP): shuffle frame, random hexes, A-B-C number placement counterclockwise skipping desert, snake placement Round 1 in order / Round 2 in reverse, 1 resource per adjacent hex of 2nd settlement.

## Turn Overview

Clockwise, 2 phases in order:

### 1. Production Phase
1. Optionally play **1 development card** before rolling (see below).
2. Roll 2d6.
3. If 2–12 (not 7): hexes with matching number produce. Each adjacent settlement → 1 card of hex resource, each adjacent city → 2 cards.
   - Robber hex produces nothing.
   - Shortage rule: if supply lacks enough for everyone, no one gets that resource; if only one player affected, they get what remains.
4. If 7: no production, instead:
   a. **Discard:** every player with >7 resource cards in hand discards half rounded down to supply.
   b. **Activate robber:** active player must move robber to a different hex, then steal 1 random resource card from a player with a building on that hex (choose victim if multiple).

### 2. Action Phase
In any order, as often as affordable:
- Trade
- Build (roads / settlements / cities / dev cards)
- Play 1 development card if none played pre-roll (VP exception below)

Then pass dice left if not won.

## Trading

Only active player trades; others may only trade with active player, not among themselves or with supply.

- Player–player: announce give/want, negotiate. No free gifts. No like-for-like swaps (e.g. 3 ore for 1 ore).
- Bank 4:1: give 4 same → take 1 different.
- Port 3:1: with building on 3:1 port, give 3 same → take 1 different.
- Port 2:1: with building on matching 2:1 port, give 2 of shown resource → take 1 different.

## Building

Pay resources to supply. Limits enforced.

- **Road (0 VP):** on empty edge. Must connect to own road/building. Cannot build past opponent building. 15 pieces.
- **Settlement (1 VP):** on empty intersection. Must satisfy Distance Rule (≥2 edges from any building) and connect to own road. 5 pieces; must upgrade to city to reuse.
- **City (2 VP):** replaces own settlement. Remove settlement, place city. 4 pieces max.
- **Development card:** draw top card, keep hidden until played.

Standard costs (verify against player-aid icons before coding):
- Road = 1 wood + 1 brick
- Settlement = 1 wood + 1 brick + 1 wool + 1 wheat
- City = 2 wheat + 3 ore
- Dev card = 1 wool + 1 wheat + 1 ore

Longest Route example (p.8): opponent settlement breaking your line splits it into segments; tile goes to now-longest eligible player.

## Development Cards

- Stay hidden until played. Not counted for 7-discard, cannot be stolen or traded. Bought cards go to hand; played cards stay face-up, never return to deck. If deck empties, no more can be built.
- Max 1 played per turn, cannot be played the turn bought — exceptions: Victory Point cards (play any number, even turn bought, only to win).
- May be played pre-roll or in Action Phase.
- Effects:
  - **Knight (14×):** move robber to new hex + steal 1 random card from building owner there. Counts toward Largest Army.
  - **Victory Point (5×):** +1 VP hidden until winning reveal.
  - **Monopoly (2×):** name 1 resource, all players give you all of that resource.
  - **Road Building (2×):** place 2 free roads (still placement-legal).
  - **Invention (2×):** take any 2 resources from supply (same or mixed).

## Bonus Tiles (2 VP each)

- **Largest Army:** first to 3 played Knights. Stolen immediately on strictly-more Knights.
- **Longest Route:** first to 5 continuous roads. Stolen immediately on strictly-more. Returned to supply if holder drops below 5 / no longer longest; re-awarded once someone has longest ≥5.

## Winning

Check on active player's turn only. Count: settlements (1) + cities (2) + VP cards (revealed to win) + bonuses (2 each). First to ≥10 ends game immediately.

## Implementation Notes for rust-catan

- Current `FieldType` / `Resource` / `RoadNodeType` names in code do NOT match this file — see `PLAN.md` M0 for renames.
- Board must enforce: 19 hexes, 18 numbers, robber-on-desert start, 54 intersections / 72 edges adjacency, distance rule, connectivity, piece counts, supply counts.
- Server is authoritative for dice, deck shuffle, robber steal randomness, VP math, turn order.
