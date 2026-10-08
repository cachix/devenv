with Ada.Text_IO; use Ada.Text_IO;
with GNATCOLL.Tribooleans; use GNATCOLL.Tribooleans;

procedure Hello_World is
    Tri: constant Triboolean = Indeterminate;
begin
    Put_Line("Hello, " & Tri'Image & " world!");
end Hello_World;

