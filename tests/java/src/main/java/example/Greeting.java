package example;

public record Greeting(String name) {
    public String message() {
        return "Hello " + name;
    }
}
