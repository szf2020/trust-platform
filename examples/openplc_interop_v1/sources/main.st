PROGRAM Main
VAR
    PulseInput : BOOL := FALSE;
    Rising     : R_TRIG;
    RisingHits : DINT := 0;
END_VAR

PulseInput := NOT PulseInput;
Rising(CLK := PulseInput);

IF Rising.Q THEN
    RisingHits := RisingHits + 1;
END_IF;
END_PROGRAM
