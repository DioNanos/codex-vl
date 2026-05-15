# Vivling CRT face-state sprites

Date: 2026-05-15
Scope: compact 3-line CRT sprites for state-face refresh.

Rules:
- ASCII only.
- 3 lines per sprite.
- State marker belongs inside the face/core, never appended on the side.
- Body/silhouette stays stable enough to avoid CRT jitter.
- Syllo, Orchestra, and Chronosworn are runtime companion candidates.
- ZED is archived for lore/panel reference; ZED is the narrator/presenter, not a normal lifecycle companion.
- Card ASCII is out of scope here.

State core legend:
- idle: `o`
- work/cycle/direct: `<`
- think/observe: `?`
- happy/balanced/source: `^`
- sleep/hold/dormant: `-`
- alert: `!`
- success/complete: `v`
- error/broken: `x`

## Syllo

### Baby

```ascii
IDLE
  .-o-.
 /(   )\
  /_<_\

WORK
  .-<-.
 /(   )\
  /_<_\

THINK
  .-?-.
 /(   )\
  /_<_\

HAPPY
  .-^-.
 /(^ ^)\
  /_<_\

SLEEP
  .---.
 /(- -)\
  /_<_\

ALERT
  .-!-.
 /( o )\
  /_<_\

SUCCESS
  .-v-.
 /(^o^)\
  /_<_\

ERROR
  .-x-.
 /( x )\
  /_<_\
```

### Juvenile

```ascii
IDLE
   .o.
  /(_)\
 _/|<|\_

WORK
   .<.
  /(_)\
 _/|<|\_

THINK
   .?.
  /(_)\
 _/|<|\_

HAPPY
   .^.
  /(^)\
 _/|<|\_

SLEEP
   ...
  /(-)\
 _/|<|\_

ALERT
   .!.
  /(o)\
 _/|<|\_

SUCCESS
   .v.
  /(^)\
 _/|<|\_

ERROR
   .x.
  /(x)\
 _/|<|\_
```

### Adult

```ascii
IDLE
  \.o./
 --/|\--
  _/ \_

WORK
  \.<./
 --/|\--
  _/ \_

THINK
  \.?./
 --/|\--
  _/ \_

HAPPY
  \.^./
 --/|\--
  _/ \_

SLEEP
  \.../
 --/|\--
  _/ \_

ALERT
  \.!./
 --/|\--
  _/ \_

SUCCESS
  \.v./
 --/|\--
  _/ \_

ERROR
  \.x./
 --/|\--
  _/ \_
```

## Orchestra

### Baby

```ascii
IDLE
  .-o-.
 -(   )-
  /___\

DIRECT
  .-<-.
 -(   )-
  /___\

THINK
  .-?-.
 -(   )-
  /___\

HAPPY
  .-^-.
 -(^o)^-
  /___\

REST
  .---.
 -(-o)-
  /___\

ALERT
  .-!-.
 -( Oo)-
  /___\

COMPLETE
  .-v-.
 -(^o)^-
  /___\

ERROR
  .-x-.
 -( xo)-
  /___\
```

### Juvenile

```ascii
IDLE
   .o.
 -<( )>-
  _/ \_

DIRECT
   .<.
 -<( )>-
  _/o\_

THINK
   .?.
 -<( )>-
  _/ \_

HAPPY
   .^.
 -<(^)>-
  _/ \_

REST
   ...
 -<(-)>-
  _/ \_

ALERT
   .!.
 -<(O)>-
  _/ \_

COMPLETE
   .v.
 -<(^)>-
  _/ \_

ERROR
   .x.
 -<(x)>-
  _/ \_
```

### Adult

```ascii
IDLE
  \.o./
 o-/|\-o
  _/ \_

DIRECT
  \.<./
 o=/|\=o
  _/ \_

THINK
  \.?./
 o-/|\-o
  _/ \_

HAPPY
  \.^./
 o-/|\-o
  _/ \_

REST
  \.../
 o-/|\-o
  _/ \_

ALERT
  \.!./
 o-/|\-o
  _/ \_

COMPLETE
  \.v./
 o-/|\-o
  _/ \_

ERROR
  \.x./
 o-/|\-o
  _/ \_
```

## Chronosworn

Chronosworn uses a time-core face. The core marker sits inside the clock/sigil, not beside it.

### Baby

```ascii
IDLE
   .o.
  ( | )
  -/_\-

CYCLE
   .<.
  ( | )
  -/_\-

OBSERVE
   .?.
  ( | )
  -/_\-

BALANCED
   .^.
  ( | )
  -/_\-

HOLD
   ...
  ( | )
  -/_\-

ALERT
   .!.
  ( | )
  -/_\-

COMPLETE
   .v.
  ( | )
  -/_\-

ERROR
   .x.
  ( | )
  -/_\-
```

### Juvenile

```ascii
IDLE
  .-o-.
 --/|\--
  o/ \o

CYCLE
  .-<-.
 =-/|\--
  o/ \o

OBSERVE
  .-?-.
 --/|\--
  o/ \o

BALANCED
  .-^-.
 --/|\--
  o/ \o

HOLD
  .---.
 --/|\--
  o/ \o

ALERT
  .-!-.
 --/|\--
  o/ \o

COMPLETE
  .-v-.
 --/|\--
  o/ \o

ERROR
  .-x-.
 --/|\--
  o/ \o
```

### Adult

```ascii
IDLE
  o-.-o
 --/|\--
 o_/ \_o

CYCLE
  o-<-o
 --/|\--
 o_/ \_o

OBSERVE
  o-?-o
 --/|\--
 o_/ \_o

BALANCED
  o-^-o
 --/|\--
 o_/ \_o

HOLD
  o---o
 --/|\--
 o_/ \_o

ALERT
  o-!-o
 --/|\--
 o_/ \_o

COMPLETE
  o-v-o
 --/|\--
 o_/ \_o

ERROR
  o-x-o
 --/|\--
 o_/ \_o
```

## ZED archive

ZED is documented here for visual continuity with the archive assets. Runtime policy: ZED is the narrator/presenter (`ZED THE PRIME`), not a regular Baby/Juvenile/Adult lifecycle companion.

### Baby / signal seed

```ascii
IDLE
  ((o))
  -( )-
   /_\

SIGNAL
  ((<))
  -( )-
   /_\

SCAN
  ((?))
  -( )-
   /_\

SOURCE
  ((^))
  -(^)-
   /_\

DORMANT
  ((-))
  -(-)-
   /_\

ALERT
  ((!))
  -(O)-
   /_\

COMPLETE
  ((v))
  -(^)-
   /_\

ERROR
  ((x))
  -(x)-
   /_\
```

### Juvenile / signal body

```ascii
IDLE
   /o\
  <( )>
  _/ \_

SIGNAL
   /<\
  <( )>
  _/ \_

SCAN
   /?\
  <( )>
  _/ \_

SOURCE
   /^\
  <(^)>
  _/ \_

DORMANT
   /-\
  <(-)>
  _/ \_

ALERT
   /!\
  <(O)>
  _/ \_

COMPLETE
   /v\
  <(^)>
  _/ \_

ERROR
   /x\
  <(x)>
  _/ \_
```

### Adult / prime signal

```ascii
IDLE
  \ o /
 --( )--
 _/ | \_

SIGNAL
  \ < /
 --( )--
 _/ | \_

SCAN
  \ ? /
 --( )--
 _/ | \_

SOURCE
  \ ^ /
 --(^)--
 _/ | \_

DORMANT
  \ - /
 --(-)--
 _/ | \_

ALERT
  \ ! /
 --(O)--
 _/ | \_

COMPLETE
  \ v /
 --(^)--
 _/ | \_

ERROR
  \ x /
 --(x)--
 _/ | \_
```

---

Per aspera ad astra.
