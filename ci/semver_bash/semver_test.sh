#!/usr/bin/env bash

. ./semver.sh

semverTest() {
local A=R1.3.2
local B=R2.3.2
local C=R1.4.2
local D=R1.3.3
local E=R1.3.2a
local F=R1.3.2b
local G=R1.2.3

local MAJOR=0
local MINOR=0
local PATCH=0
local SPECIAL=""

semverParseInto $A MAJOR MINOR PATCH SPECIAL
echo "$A -> M:$MAJOR m:$MINOR p:$PATCH s:$SPECIAL. Expect M:1 m:3 p:2 s:"
semverParseInto $E MAJOR MINOR PATCH SPECIAL
echo "$E -> M:$MAJOR m:$MINOR p:$PATCH s:$SPECIAL. Expect M:1 m:3 p:2 s:a"

echo "Equality comparisions"
semverEQ $A $A
echo "$A == $A -> $?. Expect 0."

semverLT $A $A
echo "$A < $A -> $?. Expect 1."

semverGT $A $A
echo "$A > $A -> $?. Expect 1."


echo "Major number comparisions"
semverEQ $A $B
echo "$A == $B -> $?. Expect 1."

semverLT $A $B
echo "$A < $B -> $?. Expect 0."

semverGT $A $B
echo "$A > $B -> $?. Expect 1."

semverEQ $B $A
echo "$B == $A -> $?. Expect 1."

semverLT $B $A
echo "$B < $A -> $?. Expect 1."

semverGT $B $A
echo "$B > $A -> $?. Expect 0."


echo "Minor number comparisions"
semverEQ $A $C
echo "$A == $C -> $?. Expect 1."

semverLT $A $C
echo "$A < $C -> $?. Expect 0."

semverGT $A $C
echo "$A > $C -> $?. Expect 1."

semverEQ $C $A
echo "$C == $A -> $?. Expect 1."

semverLT $C $A
echo "$C < $A -> $?. Expect 1."

semverGT $C $A
echo "$C > $A -> $?. Expect 0."

echo "patch number comparisions"
semverEQ $A $D
echo "$A == $D -> $?. Expect 1."

semverLT $A $D
echo "$A < $D -> $?. Expect 0."

semverGT $A $D
echo "$A > $D -> $?. Expect 1."

semverEQ $D $A
echo "$D == $A -> $?. Expect 1."

semverLT $D $A
echo "$D < $A -> $?. Expect 1."

semverGT $D $A
echo "$D > $A -> $?. Expect 0."

echo "special section vs no special comparisions"
semverEQ $A $E
echo "$A == $E -> $?. Expect 1."

semverLT $A $E
echo "$A < $E -> $?. Expect 1."

semverGT $A $E
echo "$A > $E -> $?. Expect 0."

semverEQ $E $A
echo "$E == $A -> $?. Expect 1."

semverLT $E $A
echo "$E < $A -> $?. Expect 0."

semverGT $E $A
echo "$E > $A -> $?. Expect 1."

echo "special section vs special comparisions"
semverEQ $E $F
echo "$E == $F -> $?. Expect 1."

semverLT $E $F
echo "$E < $F -> $?. Expect 0."

semverGT $E $F
echo "$E > $F -> $?. Expect 1."

semverEQ $F $E
echo "$F == $E -> $?. Expect 1."

semverLT $F $E
echo "$F < $E -> $?. Expect 1."

semverGT $F $E
echo "$F > $E -> $?. Expect 0."

echo "Minor and patch number comparisons"
semverEQ $A $G
echo "$A == $G -> $?. Expect 1."

semverLT $A $G
echo "$A < $G -> $?. Expect 1."

semverGT $A $G
echo "$A > $G -> $?. Expect 0."

semverEQ $G $A
echo "$G == $A -> $?. Expect 1."

semverLT $G $A
echo "$G < $A -> $?. Expect 0."

semverGT $G $A
echo "$G > $A -> $?. Expect 1."
}

semverTest
