FUNCTION_BLOCK FB_Pump
VAR_INPUT
    Command : ST_PumpCommand;
END_VAR
VAR_OUTPUT
    Status : ST_PumpStatus;
END_VAR
VAR
    ramp_timer : TON;
    ramp : REAL;
END_VAR
VAR CONSTANT
    RAMP_TIME : TIME := T#1s;
END_VAR

Status.Running := FALSE;
Status.Alarm := 0;

IF NOT Command.Enable THEN
    Status.State := E_PumpState#Idle;
    Status.ActualSpeed := 0.0;
    ramp := 0.0;
    ramp_timer(IN := FALSE);
ELSE
    CASE Status.State OF
        E_PumpState#Idle:
            Status.State := E_PumpState#Starting;
            ramp_timer(IN := TRUE, PT := RAMP_TIME);
        E_PumpState#Starting:
            ramp_timer(IN := TRUE, PT := RAMP_TIME);
            IF ramp_timer.Q THEN
                Status.State := E_PumpState#Running;
            ELSE
                ramp := ramp + 0.2;
            END_IF
        E_PumpState#Running:
            Status.Running := TRUE;
            ramp := Command.TargetSpeed;
        E_PumpState#Fault:
            Status.Running := FALSE;
            Status.Alarm := 16#BEEF;
    END_CASE
END_IF

Status.ActualSpeed := ramp;
END_FUNCTION_BLOCK
