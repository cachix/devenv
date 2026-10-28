package example;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.DataInputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;

class RuntimeTest {
    @Test
    void testWorkerUsesProjectJdk() throws Exception {
        assertEquals(17, Runtime.version().feature());
        assertTrue(Files.isSameFile(
            Path.of(System.getProperty("java.home"), "bin", "java"),
            Path.of(System.getenv("JAVA_HOME"), "bin", "java")));
        System.out.println("Test JVM: " + System.getProperty("java.home"));
    }

    @Test
    void compilerProducesJava17Bytecode() throws Exception {
        try (var in = new DataInputStream(Greeting.class.getResourceAsStream("Greeting.class"))) {
            assertEquals(0xCAFEBABE, in.readInt());
            in.readUnsignedShort();
            assertEquals(61, in.readUnsignedShort());
        }
    }

    @Test
    void compiledCodeWorks() {
        assertEquals("Hello devenv", new Greeting("devenv").message());
    }
}
