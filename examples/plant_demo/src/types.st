TYPE
    E_PumpState : (Idle := 0, Starting := 1, Running := 2, Fault := 3);

    ST_PumpCommand :
    STRUCT
        Enable : BOOL;
        TargetSpeed : REAL;
    END_STRUCT;

    ST_PumpStatus :
    STRUCT
        Running : BOOL;
        State : E_PumpState;
        ActualSpeed : REAL;
        Alarm : UDINT;
    END_STRUCT;
END_TYPE
