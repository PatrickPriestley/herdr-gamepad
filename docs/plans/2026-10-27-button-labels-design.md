# Clear controller button labels

The user confirmed the existing close sequence works: Square selects a pane and Cross confirms. Physical-position wording made the popup look like a D-pad menu. Keep the bindings and close-state safety unchanged; replace the visible popup and mapping instructions with PlayStation names and explicitly qualified Xbox equivalents.

Use “Square (Xbox: X): Close pane”, “Triangle (Xbox: Y): Close space”, “Cross (Xbox: A): Confirm”, and “Circle (Xbox: B): Cancel”. The other cancellation controls read “Create / Share (Xbox: View)” and “Options (Xbox: Menu)”; voice-on-Menu retains “L3 / left stick click”. Each action has its own line. Automatic controller detection would broaden this copy fix; bare slash pairs would leave PlayStation Cross and Xbox X ambiguous.

Probed facts: the installed and running build is 0865c55, which contains the close menu. The observed trace had Options, D-pad and Cross but no Square selection; the user then confirmed that distinction. The popup manifest is 64 columns by 12 rows. Scratch tests exercise both confirmation stages and all three voice configurations; representative target IDs fit within 62 interior columns and ten interior rows. D-pad behavior, command dispatch, logging names, and closed-target protection are unchanged.

The mapping's introductory physical-position table remains useful for other controllers, but its action instructions use the same explicit names as the popup. README changes are restricted to controller instructions; release/build work in other branches is untouched.
