PROGRAM Main
VAR
    RisingSeen : BOOL := FALSE;
    DI0 : BOOL := FALSE;
    DO0 : BOOL := FALSE;
END_VAR

IF DI0 THEN
    RisingSeen := TRUE;
END_IF;

DO0 := DI0 OR RisingSeen;
END_PROGRAM
