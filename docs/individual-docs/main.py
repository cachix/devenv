import os

# List all files in the "languages" directory

new_content = '''

[comment]: # (Please add your documentation on top of this line)

@AUTOGEN_OPTIONS@
'''
for dirName in [ "languages",  "services", "process-managers"]:
    for file in os.listdir(dirName):
        # Construct the full file path
        file_path = os.path.join(dirName, file)
        # Open the file in read/write mode
        with open(file_path, "w") as f:
            f.write(new_content)

