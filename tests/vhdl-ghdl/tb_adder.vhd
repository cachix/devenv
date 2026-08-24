library ieee;
use ieee.std_logic_1164.all;

entity tb_adder is
end entity;

architecture test of tb_adder is
    signal a : std_logic_vector(7 downto 0) := (others => '0');
    signal b : std_logic_vector(7 downto 0) := (others => '0');
    signal sum : std_logic_vector(7 downto 0);
begin
    uut: entity work.adder port map (a, b, sum);
    
    process
    begin
        a <= X"05";
        b <= X"03";
        wait for 10 ns;
        assert sum = X"08" report "Adder failed!" severity error;
        a <= X"FF";
        b <= X"01";
        wait for 10 ns;
        assert sum = X"00" report "Overflow wrap-around failed!" severity error;
        report "Test completed successfully";
        wait;
    end process;
end architecture;
