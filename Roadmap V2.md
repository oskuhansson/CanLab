I've created this project in need of a robust cas/simulation tool for my can sat project. I realise that the current state of the project is nowhere near capable enough for my needs and requires iteration. In this roadmap ill list the needed capabilities and the workflow that im looking for. Im also dissapointed by the lack of ambiton on your part. We were able to create this state of the projekt in just 2 days i think we can accomplish quite a lot more in the coming weeks.
This roadmap is quite ambitous so im expecting many iterations where features get tested and refined before continuing.

## Capabilities

### Basic math
I want you to be able to use the cas to help me solve upper secondary school level problems. You can find these kinds of problems at: https://svenska.yle.fi/abimix/matematik. The cas should be able to use to solve nearly all of these problems.

### Simulation
The cas should be able to simulate physical pheomenon through time by estimating diffrential equations using solvers like euler and rk4. For exampel im planning on simulating the path light takes in a block of glass. Performace is critical witch is why im thinking of creating a compiler that translates the expressions to be evalutaed into gpu code for the simulation to instead be runned there.

### Visualisation
The cas should have both a 2d and 3d view.

### Code support
The cas should have support for interpreted languages like python or lua. Could look in to other ones that fit better.

## Workflow
The workflow should be smooth and hassel free. Try having most capabilitise like plotting and solving happen mostly in the background instead of by the press of a button. I simply want to be able to type in my equation and be given solutions.
### Autofill
When i'm typing in the equation x= into a system that contains x^2+y^2=1 i want the solver to autofill the solution sqrt(1-y^2) so i can fill that in with tab.

### Simulation
Simulating should simply consist of typing in the diffrential equations. Staring point (in the form of equations) and then the estimator backend would handle getting estimated solutions. Being able to solve some diffrential equations would also be good but for our purpsoes its enough to be able to rework them into shapes that are easily simulatable.

### Testing
When testing the project i noticed quite to many bugs or uniplmented features that ide like to. Create more robust testing by creating more comprehensive cases. Use problems from:  https://svenska.yle.fi/abimix/matematik. And also for the simulation construct simulations of water, air, light and sound. The test cases should be uniqe and complex testing a feature to its breaking point, quality>quantity. But dont be afraid to create more cases. Tesing should also be done in batches. Try figuring out a way to get screenshots of the ui without opning a windoe, since its annoying on my part.

## Bevy
I think its time to move to bevy. Compile times shouldnt actually be that bad with dynamic linking. Also if the app is to slow add higher optimization for non development packages.
