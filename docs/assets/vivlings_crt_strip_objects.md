# Vivlings / Codex-VL - CRT Strip Objects

Blocchi testuali copiabili per oggetti usabili nella striscia CRT compatta a 3 righe.

---

## PLAY / IDLE objects

```text
# BALL - base
   o
 ---

# BALL - bounce frames
   o        .o.        o
 ---       ---       ---

# BALL - use with Vivling
 (o_o)  ...>  o
 /___\      ---
```

```text
# CUBE - base
  []
 ----

# CUBE - anim frames
  []       [/]      [*]      [\]      []
 ----     ----     ----     ----     ----

# CUBE - use with Vivling
 (o_o)  ...>  []
 /___\      ----
```

```text
# CRT_ORB - base
  (@)
 ----

# CRT_ORB - pulse frames
  (o)      ((o))     (((o)))    ((o))     (o)
 ----      ----       ----      ----     ----

# CRT_ORB - use with Vivling
 (o_o)  ..>  ((o))
 /___\       ----
```

```text
# SPARK_STAR - base
   +
  ---

# SPARK_STAR - twinkle frames
   +        *        *        *        +
  ---      ---      ---      ---      ---

# SPARK_STAR - use with Vivling
 (o_o)  ..>  *
 /___\      ---
```

---

## REST / CARE objects

```text
# BLANKET - base
  __
 /_/

# BLANKET - fold frames
  __       _~_       ___       __
 /_/      /_/       /___\     /_/

# BLANKET - use with Vivling
 (-_-)  ..>  __
 /___\      /_/
```

```text
# NEST - base
 \___/
  \_/

# NEST - glow/rest frames
 \___/    \_*_/    \_o_/    \_*_/    \___/
  \_/      \_/      \_/      \_/      \_/

# NEST - use with Vivling
 (-_-)  ..> \___/
 /___\       \_/
```

```text
# SNACK - base
  (.)
 ----

# SNACK - eat frames
  (.)      (c)      c        .        *
 ----     ----     ----     ----     ----

# SNACK - use with Vivling
 (o_o)  ..>  (.)
 /___\       ----
```

```text
# PILLOW - base
  ___
 (___)

# PILLOW - puff frames
  ___      _~_      ___      _^_      ___
 (___)   (___)    (___)    (___)    (___)

# PILLOW - use with Vivling
 (-_-)  zZ   ___
 /___\      (___)
```

---

## WORK / CODE objects

```text
# LOGBOOK - base
  []
 /__\

# LOGBOOK - open frames
  []      [ ]      [*]      [ ]      []
 /__\    /___\    /___\    /___\    /__\

# LOGBOOK - use with Vivling
 (o_o)  ...  [ ]
 /___\      /___\
```

```text
# MINI_TERMINAL - base
  [_]
 /___\

# MINI_TERMINAL - blink frames
  [_]      [.]      [>]      [.]      [_]
 /___\    /___\    /___\    /___\    /___\

# MINI_TERMINAL - use with Vivling
 (o_o)  ...  [>]
 /___\      /___\
```

```text
# TOOL - base
  Y
 /|

# TOOL - spin frames
  Y       \Y/       Y       /Y\       Y
 /|        |       /|        |       /|

# TOOL - use with Vivling
 (o_o)  ...  Y
 /___\      /|
```

```text
# TEST_CHIP - base
 [#]
-===-

# TEST_CHIP - scan frames
 [#]     ((#))    [#]v    ((#))    [#]
-===-    -===-    -===-    -===-    -===-

# TEST_CHIP - use with Vivling
 (o_o)  ... [#]v
 /___\     -===-
```

---

## SIGNAL / ARCANE objects

```text
# MEMORY_SHARD - base
  <>
  ||

# MEMORY_SHARD - glow frames
  <>      <*>      <O>      <*>      <>
  ||       ||       ||       ||      ||

# MEMORY_SHARD - use with Vivling
 (o_o)  ..>  <>
 /___\       ||
```

```text
# SCAN_LENS - base
  <o>
 ----

# SCAN_LENS - scan frames
  <o>     (o)>>>   <O>     <<<o)    <o>
 ----     ----     ----     ----    ----

# SCAN_LENS - use with Vivling
 (o_o)-o  ...  []
 /___\        ----
```

```text
# SIGNAL_KEY - base
  o-
  |

# SIGNAL_KEY - signal frames
  o-      (o-)     ((o-))    (o-)     o-
  |        |         |        |       |

# SIGNAL_KEY - use with Vivling
 (o_o)  ..>  o-  > []
 /___\       |
```

```text
# LOG_LANTERN - base
  []
 [**]
  ||

# LOG_LANTERN - flicker frames
  []      []      []      []      []
 [.*]    [**]    [##]    [**]    [.*]
  ||      ||      ||      ||      ||

# LOG_LANTERN - use with Vivling
 (o_o)  ...  []
 /___\      [**]
             ||
```

---

## Canonical strip examples

```text
     _|\        []
   ( >_> ) ... [**]   watching logs glow
    /___\       ||
```

```text
     _|\       [>]
   ( o_o ) ... /___\   reading terminal output
    /___\
```

```text
     _|\        o
   ( >_> )  ..>---    bouncing during idle
    /___\
```

---

## Suggested data model

```text
object_id:
  name:
  category:
  tags:
  base:
  frames:
  use_frames:
  notes:
```
