entity hello_world is
end entity;

architecture arc of hello_world is
begin
  assert false report "Hello, World!" severity note;
end architecture;
