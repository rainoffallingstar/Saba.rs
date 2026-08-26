# Ryusei Domain Glossary

## Session mode

The user's top-level purpose for an open game. `Match` is a local or remote
competitive game, `Record` is studying or editing an SGF, and `Live` is a
read-only observation of a broadcast.

## Board interaction mode

The immediate board tool in use, such as play, edit, scoring, or estimation.
It is independent from session mode: scoring can happen during a Match after
both players pass, while editing belongs only to Record.

## Analysis policy

The permitted engine-analysis behavior for a session. `Off` has no automatic
analysis, `Manual` requires a user request, `Continuous` follows position
updates, and `FairPlayLockedOff` prohibits analysis because a human remote
competition is in progress.

## Time control

The agreement governing time use in a Match. The first supported controls are
no clock, absolute main time, and Japanese byo-yomi consisting of main time,
period duration, and a number of periods.

## Clock

The current time state for both players. A clock is locally predictive for a
local Match and is server-authoritative for a remote Match.

## Chinese ancient rules

An area-scoring historical rules preset that applies a two-point group tax
(还棋头) to every surviving connected group. It is distinct from modern Chinese
rules. The optional four-corner seat-stone opening convention (座子制) is an
opening convention, not a scoring rule.

## Review profile

A bounded full-game analysis budget. The named profiles are Quick (50 visits),
Preliminary (800), Intermediate (2500), and Advanced (10000). It is separate
from the interactive analysis depth selected for the currently viewed position.

## HumanSL profile

The rank and era target for KataGo's HumanSL model. A profile has either the
modern `rank_` or historical `preaz_` family, followed by a rank from 20K
through 9D. It affects human-like move selection and is distinct from the
strength of the normal KataGo search model.
